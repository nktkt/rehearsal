use std::{
    fs,
    time::{Duration, SystemTime},
};

use rehearsal::{
    config::{Assertion, Config},
    report::{self, Status},
    runner,
};
use rusqlite::Connection;
use tempfile::TempDir;

fn fixture() -> (TempDir, Config) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("backup.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
        CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE notes(id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id));
        INSERT INTO users VALUES(1, 'Ada');
        INSERT INTO notes VALUES(1, 1);",
        )
        .unwrap();
    connection.close().unwrap();
    let mut config = Config::for_backup(path);
    config.report_dir = directory.path().join("reports");
    (directory, config)
}

fn sql(name: &str, query: &str) -> Assertion {
    Assertion {
        name: name.into(),
        sql: query.into(),
    }
}

#[test]
fn restores_a_read_only_snapshot_without_changing_the_source() {
    let (_directory, mut config) = fixture();
    let before = fs::read(&config.backup).unwrap();
    let modified = config.backup.metadata().unwrap().modified().unwrap();
    let mut permissions = config.backup.metadata().unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&config.backup, permissions).unwrap();
    config.max_age = Some("1h".into());
    config
        .checks
        .push(sql("Users survived", "SELECT COUNT(*) = 1 FROM users"));
    let result = runner::run(&config).unwrap();
    assert_eq!(result.status, Status::Passed, "{}", result.render());
    assert_eq!(result.restored_bytes, Some(before.len() as u64));
    assert_eq!(fs::read(&config.backup).unwrap(), before);
    assert_eq!(
        config.backup.metadata().unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        fs::read_dir(config.backup.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn reports_missing_empty_non_sqlite_and_truncated_backups_as_failures() {
    let (directory, _) = fixture();
    for (index, bytes) in [
        vec![],
        b"not a sqlite database".to_vec(),
        b"SQLite format 3\0broken pages".to_vec(),
    ]
    .iter()
    .enumerate()
    {
        let path = directory.path().join(format!("bad-{index}.sqlite"));
        fs::write(&path, bytes).unwrap();
        let report = runner::run(&Config::for_backup(path)).unwrap();
        assert_eq!(report.status, Status::Failed, "{}", report.render());
    }
    let missing = directory.path().join("missing.sqlite");
    let result = runner::run(&Config::for_backup(missing.clone())).unwrap();
    assert_eq!(result.status, Status::Failed);
    assert!(
        !missing.exists(),
        "opening a missing backup must not create an empty database"
    );
}

#[test]
fn detects_foreign_key_violations_that_integrity_check_does_not() {
    let (_directory, config) = fixture();
    let connection = Connection::open(&config.backup).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys = OFF; INSERT INTO notes VALUES(2, 999);")
        .unwrap();
    connection.close().unwrap();
    let result = runner::run(&config).unwrap();
    assert_eq!(result.status, Status::Failed);
    assert_eq!(
        result
            .checks
            .iter()
            .find(|check| check.name == "SQLite integrity")
            .unwrap()
            .status,
        Status::Passed
    );
    assert_eq!(
        result
            .checks
            .iter()
            .find(|check| check.name == "Foreign keys")
            .unwrap()
            .status,
        Status::Failed
    );
}

#[test]
fn refuses_live_wal_databases_instead_of_omitting_recent_transactions() {
    let (_directory, config) = fixture();
    let connection = Connection::open(&config.backup).unwrap();
    connection
        .execute_batch("PRAGMA journal_mode = WAL; INSERT INTO users VALUES(2, 'Grace');")
        .unwrap();
    let result = runner::run(&config).unwrap();
    assert_eq!(result.status, Status::Failed);
    assert!(result.checks[0].detail.contains("standalone backup"));
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn refuses_all_sidecars_even_when_empty() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let (_directory, config) = fixture();
        let sidecar = format!("{}{suffix}", config.backup.display());
        fs::write(sidecar, []).unwrap();
        assert_eq!(runner::run(&config).unwrap().status, Status::Failed);
    }
}

#[test]
fn rejects_failed_ambiguous_and_writable_assertions() {
    let (_directory, mut config) = fixture();
    let before = fs::read(&config.backup).unwrap();
    let invalid_queries = [
        "SELECT 0",
        "SELECT NULL",
        "SELECT '1'",
        "SELECT 2",
        "SELECT 1.0",
        "SELECT 1 WHERE 0",
        "SELECT 1, 1",
        "SELECT 1 UNION ALL SELECT 1",
        "SELECT 1; SELECT 0",
        "DELETE FROM users RETURNING 1",
        "PRAGMA user_version = 1",
        "ATTACH ':memory:' AS another",
        "SELECT * FROM missing_table",
    ];
    for (index, query) in invalid_queries.iter().enumerate() {
        config.checks.push(sql(&format!("invalid-{index}"), query));
    }
    config
        .checks
        .push(sql("Final read", "SELECT COUNT(*) = 1 FROM users"));
    let result = runner::run(&config).unwrap();
    for (index, query) in invalid_queries.iter().enumerate() {
        let name = format!("SQL: invalid-{index}");
        let check = result
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap();
        assert_eq!(check.status, Status::Failed, "unexpected pass for {query}");
    }
    assert_eq!(
        result
            .checks
            .iter()
            .find(|check| check.name == "SQL: Final read")
            .unwrap()
            .status,
        Status::Passed
    );
    assert_eq!(fs::read(&config.backup).unwrap(), before);
}

#[test]
fn interrupts_a_long_query_and_still_runs_the_next_assertion() {
    let (_directory, mut config) = fixture();
    config.timeout = "100ms".into();
    config.checks.push(sql(
        "Slow",
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x + 1 FROM n) SELECT SUM(x) = 1 FROM n",
    ));
    config.checks.push(sql("Next", "SELECT 1"));
    let result = runner::run(&config).unwrap();
    let slow = result
        .checks
        .iter()
        .find(|check| check.name == "SQL: Slow")
        .unwrap();
    assert_eq!(slow.status, Status::Failed);
    assert!(slow.detail.contains("timeout"), "{}", slow.detail);
    assert_eq!(
        result
            .checks
            .iter()
            .find(|check| check.name == "SQL: Next")
            .unwrap()
            .status,
        Status::Passed
    );
}

#[test]
fn optional_age_check_rejects_old_or_future_file_timestamps() {
    for timestamp in [
        SystemTime::now() - Duration::from_secs(7200),
        SystemTime::now() + Duration::from_secs(7200),
    ] {
        let (_directory, mut config) = fixture();
        fs::File::options()
            .write(true)
            .open(&config.backup)
            .unwrap()
            .set_modified(timestamp)
            .unwrap();
        config.max_age = Some("1h".into());
        let result = runner::run(&config).unwrap();
        assert_eq!(result.status, Status::Failed);
        assert_eq!(
            result
                .checks
                .iter()
                .find(|check| check.name == "Backup age")
                .unwrap()
                .status,
            Status::Failed
        );
        assert_eq!(
            result
                .checks
                .iter()
                .find(|check| check.name == "SQLite integrity")
                .unwrap()
                .status,
            Status::Passed
        );
    }
}

#[test]
fn reports_are_unique_and_history_includes_failed_runs() {
    let (_directory, mut config) = fixture();
    let success = runner::run(&config).unwrap();
    let first = success.save(&config.report_dir).unwrap();
    let second = success.save(&config.report_dir).unwrap();
    assert_ne!(first, second);
    config.checks.push(sql("Fails", "SELECT 0"));
    runner::run(&config)
        .unwrap()
        .save(&config.report_dir)
        .unwrap();
    let records = report::history(&config.report_dir, 2).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].status, Status::Failed);
    assert_eq!(records[1].status, Status::Passed);
    assert_eq!(report::history(&config.report_dir, 20).unwrap().len(), 3);
}

#[test]
fn paths_are_relative_to_config_and_configuration_errors_are_not_ignored() {
    let (directory, _) = fixture();
    let path = directory.path().join("rehearsal.toml");
    fs::write(&path, "backup = 'backup.sqlite'\nreport_dir = 'reports'\n").unwrap();
    let config = Config::load(&path).unwrap();
    assert_eq!(config.backup, directory.path().join("backup.sqlite"));
    assert_eq!(config.report_dir, directory.path().join("reports"));
    assert_eq!(runner::run(&config).unwrap().status, Status::Passed);
    for contents in [
        "backup = ''",
        "backup = 'a'\nreport_dir = ''",
        "backup = 'a'\nmax_gae = '24h'",
        "backup = 'a'\ntimeout = '0s'",
        "backup = 'a'\nmax_age = 'yesterday'",
        "backup = 'a'\n[[checks]]\nname = 'a'\nsql = 'SELECT 1'\n[[checks]]\nname = 'a'\nsql = 'SELECT 1'",
    ] {
        fs::write(&path, contents).unwrap();
        assert!(
            Config::load(&path).is_err(),
            "invalid configuration was accepted: {contents}"
        );
    }
    assert!(
        report::history(&directory.path().join("does-not-exist"), 10)
            .unwrap()
            .is_empty()
    );
}
