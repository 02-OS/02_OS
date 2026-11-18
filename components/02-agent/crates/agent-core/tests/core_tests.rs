use agent_core::{
    contain::{random_hex, write_private_atomic},
    truncate_chars, HostKind, Language, LiveEnvironmentInfo, RepoConfig, RepoId, SecretPattern,
    StoragePaths,
};
use std::borrow::Cow;
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

    let aws = format!("aws_secret_access_key = \"{}\"", "A".repeat(40));
    let aws_redacted = SecretPattern::redact(&aws);
    assert!(!aws_redacted.contains(&"A".repeat(40)));
    assert!(aws_redacted.contains("[REDACTED]"));
    let bare_hex = "a".repeat(40);
    assert!(SecretPattern::redact(&bare_hex).contains(&bare_hex));
}

#[test]
fn test_host_kind_uses_live_markers_and_os_release_name() {
    assert_eq!(
        HostKind::classify(true, "NAME=\"Arch Linux\"\n"),
        HostKind::Live
    );
    assert_eq!(
        HostKind::classify(false, "NAME=\"02_OS\"\nID=02os\n"),
        HostKind::Installed
    );
    assert_eq!(
        HostKind::classify(false, "NAME=02_OS\n"),
        HostKind::Installed
    );
    assert_eq!(
        HostKind::classify(false, "NAME=\"Arch Linux\"\nID=arch\n"),
        HostKind::Unidentified
    );
    assert_eq!(HostKind::classify(false, ""), HostKind::Unidentified);
    assert_eq!(HostKind::Unidentified.label(), "Unidentified host");
    assert_eq!(HostKind::Installed.label(), "02_OS Installed System");
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

#[test]
fn test_secret_redaction_extended_patterns_and_cow() {
    assert!(matches!(
        SecretPattern::redact_cow("pub fn clean_helper() -> i32 { 42 }"),
        Cow::Borrowed(_)
    ));

    let samples = [
        "aws_key=ASIATEST1234567890AB".to_string(),
        format!("gho_{}", "b".repeat(36)),
        format!("ghu_{}", "c".repeat(36)),
        format!("ghs_{}", "d".repeat(36)),
        format!("ghr_{}", "e".repeat(36)),
        format!("github_pat_{}", "f".repeat(45)),
        "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc.def".to_string(),
        "Authorization: Basic dXNlcjpwYXNzd29yZDEyMw==".to_string(),
        "https://alice:hunter2secret@git.example.com/org/repo.git".to_string(),
        r#"{"password": "base64+Secret/Value=="}"#.to_string(),
        r#"passwd = "jwt.part1.part2.part3""#.to_string(),
        r#"auth: "super+secret/token=1234""#.to_string(),
    ];

    for sample in samples {
        let out = SecretPattern::redact(&sample);
        assert!(out.contains("[REDACTED]"), "failed to redact: {sample}");
    }
    assert!(
        !SecretPattern::redact("https://alice:hunter2secret@git.example.com/repo")
            .contains("hunter2secret")
    );
    assert_eq!(
        SecretPattern::redact("https://example.com:8443/path"),
        "https://example.com:8443/path"
    );
}

#[test]
fn test_truncate_chars_and_write_private_atomic() {
    let text = "aébç🚀d";
    assert_eq!(truncate_chars(text, 3), "aéb");
    assert_eq!(truncate_chars(text, 5), "aébç🚀");
    assert_eq!(truncate_chars(text, 100), text);

    let dir = tempdir().unwrap();
    let target = dir.path().join("nested").join("state.json");
    write_private_atomic(&target, br#"{"ok":true}"#).unwrap();
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(std::fs::read(&target).unwrap(), br#"{"ok":true}"#);
    assert_eq!(random_hex(16).unwrap().len(), 32);
}
