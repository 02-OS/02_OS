use agent_core::{Language, LiveEnvironmentInfo, RepoConfig, RepoId, SecretPattern, StoragePaths};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn test_repo_id_deterministic() {
    let id1 = RepoId::from_path("/home/user/project");
    let id2 = RepoId::from_path("/home/user/project");
    assert_eq!(id1, id2);
    assert_eq!(id1.as_str().len(), 16);
}

#[test]
fn test_language_detection() {
    assert_eq!(Language::from_extension("rs"), Language::Rust);
    assert_eq!(Language::from_extension("py"), Language::Python);
    assert_eq!(Language::from_extension("ts"), Language::TypeScript);
    assert_eq!(Language::from_extension("cpp"), Language::Cpp);
    assert_eq!(Language::from_extension("dart"), Language::Dart);
    assert_eq!(Language::from_extension("sh"), Language::Bash);
    assert_eq!(Language::from_extension("unknownext"), Language::Unknown);
    assert_eq!(Language::from_name("rust"), Language::Rust);
    assert_eq!(Language::from_name("python"), Language::Python);
    assert_eq!(Language::from_name("typescript"), Language::TypeScript);
    assert_eq!(Language::from_name("javascript"), Language::JavaScript);
    assert_eq!(Language::from_name("bash"), Language::Bash);
    assert_eq!(Language::from_name("cpp"), Language::Cpp);
    assert_eq!(Language::from_name("rs"), Language::Rust);
    assert_eq!(Language::from_name("not-a-language"), Language::Unknown);
}

#[test]
fn test_secret_detection() {
    assert!(SecretPattern::is_secret(Path::new(".env")));
    assert!(SecretPattern::is_secret(Path::new(".env.local")));
    assert!(SecretPattern::is_secret(Path::new("id_rsa")));
    assert!(SecretPattern::is_secret(Path::new("server.key")));
    assert!(SecretPattern::is_secret(Path::new("certificate.pem")));
    assert!(SecretPattern::is_secret(Path::new("credentials.json")));
    assert!(!SecretPattern::is_secret(Path::new("main.rs")));
    assert!(!SecretPattern::is_secret(Path::new("README.md")));
    assert!(!SecretPattern::is_secret(Path::new("secret_guide.md")));
    assert!(!SecretPattern::is_secret(Path::new("secret_rotation.go")));
    assert!(SecretPattern::is_secret(Path::new("my_private_key.bin")));
    assert!(SecretPattern::is_secret(Path::new("secrets.json")));
    let token = format!("ghp_{}", "a".repeat(36));
    let redacted = SecretPattern::redact(&format!("prefix {token} suffix"));
    assert!(!redacted.contains(&token));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn test_live_detection_ignores_user_name() {
    let previous = std::env::var("USER").ok();
    std::env::set_var("USER", "live");
    let info = LiveEnvironmentInfo::detect();
    match previous {
        Some(value) => std::env::set_var("USER", value),
        None => std::env::remove_var("USER"),
    }
    if !Path::new("/run/archiso/bootmnt").exists() && !Path::new("/run/archiso/airootfs").exists() {
        assert!(!info.is_live);
        assert!(!info.live_overlay_detected);
    }
}

#[test]
fn test_private_dir_is_mode_0700() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("private");
    StoragePaths::ensure_private_dir(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
fn test_config_save_and_load() {
    let dir = tempdir().unwrap();
    let config = RepoConfig::default();
    config.save_to_repo(dir.path()).unwrap();

    let loaded = RepoConfig::load_or_default(dir.path());
    assert_eq!(loaded.max_file_size_kb, config.max_file_size_kb);
    assert!(loaded.exclude_secrets);

    let before = std::fs::read_to_string(dir.path().join(".02agent/config.toml")).unwrap();
    config.save_to_repo(dir.path()).unwrap();
    let after = std::fs::read_to_string(dir.path().join(".02agent/config.toml")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn test_config_refuses_symlink_and_does_not_follow_it() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let external = outside.path().join("config.toml");
    std::fs::write(&external, "max_file_size_kb = 1\n").unwrap();
    std::fs::create_dir(dir.path().join(".02agent")).unwrap();
    std::os::unix::fs::symlink(&external, dir.path().join(".02agent/config.toml")).unwrap();

    let loaded = RepoConfig::load_or_default(dir.path());
    assert_eq!(
        loaded.max_file_size_kb,
        RepoConfig::default().max_file_size_kb
    );

    let err = RepoConfig::default().save_to_repo(dir.path()).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("symlink") || err.to_string().contains("refus")
    );
    assert_eq!(
        std::fs::read_to_string(&external).unwrap(),
        "max_file_size_kb = 1\n"
    );
}
