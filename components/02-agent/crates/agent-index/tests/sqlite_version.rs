#[test]
fn bundled_sqlite_is_patched_for_cve_2026_119375() {
    let version = agent_index::sqlite_version();
    let version_number = agent_index::sqlite_version_number();
    assert!(
        version_number >= 3_054_000,
        "expected bundled SQLite >= 3.54.0 (CVE-2026-119375 / RUSTSEC-2026-0459), got {version} ({version_number})"
    );

    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let queried: String = conn
        .query_row("SELECT sqlite_version()", [], |row| row.get(0))
        .unwrap();
    assert_eq!(queried, version);
}
