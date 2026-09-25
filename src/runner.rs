use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use rusqlite::{
    Connection, OpenFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    types::ValueRef,
};

use crate::{
    config::{Assertion, Config},
    report::{CheckResult, Report, Status},
};

pub fn run(config: &Config) -> Result<Report> {
    config.validate()?;
    let started = Instant::now();
    let timeout = config.query_timeout()?;
    let max_age = config.max_backup_age()?;
    let workspace = tempfile::Builder::new()
        .prefix("rehearsal-")
        .tempdir()
        .context("cannot create temporary restore directory")?;
    let restored_path = workspace.path().join("restored.sqlite");
    let mut report = Report {
        schema_version: 1,
        tool_version: env!("CARGO_PKG_VERSION").into(),
        sqlite_version: rusqlite::version().into(),
        name: config.name.clone(),
        source: config.backup.clone(),
        started_at: Utc::now(),
        duration_ms: 0,
        status: Status::Passed,
        restored_bytes: None,
        backup_modified_at: None,
        checks: Vec::new(),
    };

    let mut snapshot = None;
    step(&mut report, "Restore", || {
        let result = restore(&config.backup, &restored_path)?;
        let detail = format!(
            "Restored {} bytes into a temporary directory.",
            result.bytes
        );
        snapshot = Some(result);
        Ok(detail)
    });

    if let Some(snapshot) = snapshot {
        report.restored_bytes = Some(snapshot.bytes);
        report.backup_modified_at = Some(snapshot.modified.into());
        if let Some(max_age) = max_age {
            step(&mut report, "Backup age", || {
                let age = SystemTime::now().duration_since(snapshot.modified)
                    .context("backup modification time is in the future; check the file timestamp and system clock")?;
                ensure!(
                    age <= max_age,
                    "backup file is {} old; limit is {}",
                    humantime::format_duration(age),
                    humantime::format_duration(max_age)
                );
                Ok(format!(
                    "File age {} (limit {}).",
                    humantime::format_duration(age),
                    humantime::format_duration(max_age)
                ))
            });
        } else {
            skip(
                &mut report,
                "Backup age",
                "No max_age configured; data freshness is not implied.",
            );
        }

        let mut connection = None;
        step(&mut report, "Open database", || {
            let database = Connection::open_with_flags(
                &restored_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            database.pragma_update(None, "query_only", true)?;
            database.pragma_update(None, "trusted_schema", false)?;
            connection = Some(database);
            Ok("Opened the restored copy read-only.".into())
        });

        if let Some(database) = connection {
            let healthy = step(&mut report, "SQLite integrity", || {
                with_timeout(&database, timeout, || integrity(&database))
            });
            if healthy {
                step(&mut report, "Foreign keys", || {
                    with_timeout(&database, timeout, || foreign_keys(&database))
                });
                // Authorize only data reads for user assertions. This also blocks ATTACH
                // and PRAGMAs, which stmt.readonly() alone would not reliably exclude.
                database.authorizer(Some(read_only_authorizer))?;
                for assertion in &config.checks {
                    step(&mut report, &format!("SQL: {}", assertion.name), || {
                        with_timeout(&database, timeout, || assert_query(&database, assertion))
                    });
                }
            } else {
                skip(
                    &mut report,
                    "Foreign keys",
                    "SQLite integrity check failed.",
                );
                skip_assertions(&mut report, config, "SQLite integrity check failed.");
            }
        } else {
            skip_database_checks(
                &mut report,
                config,
                "Restored database could not be opened.",
            );
        }
    } else {
        skip(&mut report, "Backup age", "Restore failed.");
        skip(&mut report, "Open database", "Restore failed.");
        skip_database_checks(&mut report, config, "Restore failed.");
    }

    // All SQLite handles have been dropped before removing the temporary files.
    step(&mut report, "Cleanup", || {
        workspace
            .close()
            .context("cannot remove temporary restore directory")?;
        Ok("Removed the temporary restored database.".into())
    });
    report.duration_ms = milliseconds(started.elapsed());
    report.status = if report
        .checks
        .iter()
        .any(|check| check.status == Status::Failed)
    {
        Status::Failed
    } else {
        Status::Passed
    };
    Ok(report)
}

struct Snapshot {
    bytes: u64,
    modified: SystemTime,
}

fn restore(source: &Path, destination: &Path) -> Result<Snapshot> {
    let canonical = source
        .canonicalize()
        .with_context(|| format!("cannot find backup {}", source.display()))?;
    ensure!(
        canonical.metadata()?.is_file(),
        "backup must be a regular file"
    );
    check_sidecars(&canonical)?;
    let mut input = File::open(&canonical).context("cannot read backup")?;
    let before = input.metadata()?;
    let modified = before
        .modified()
        .context("cannot read backup modification time")?;
    let mut header = [0_u8; 16];
    input
        .read_exact(&mut header)
        .context("backup is empty or truncated; expected a SQLite database")?;
    ensure!(
        &header == b"SQLite format 3\0",
        "backup does not have a SQLite 3 header; expected an uncompressed SQLite snapshot"
    );
    input.rewind()?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let bytes = std::io::copy(&mut input, &mut output)
        .context("cannot restore backup; check available disk space")?;
    output.sync_all()?;
    let after = input.metadata()?;
    ensure!(
        bytes == before.len() && before.len() == after.len() && modified == after.modified()?,
        "backup changed during the restore; use a completed snapshot"
    );
    check_sidecars(&canonical)?;
    Ok(Snapshot { bytes, modified })
}

fn check_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sibling = path.as_os_str().to_os_string();
        sibling.push(suffix);
        let sibling = PathBuf::from(sibling);
        match fs::symlink_metadata(&sibling) {
            Ok(_) => bail!(
                "found {}; use a completed standalone backup made with SQLite's backup API, .backup, or VACUUM INTO",
                sibling.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("cannot inspect {}", sibling.display()));
            }
        }
    }
    Ok(())
}

fn integrity(database: &Connection) -> Result<String> {
    let mut statement = database.prepare("PRAGMA integrity_check")?;
    let messages = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        messages.len() == 1 && messages[0] == "ok",
        "{}",
        messages.join("; ")
    );
    Ok("PRAGMA integrity_check returned ok.".into())
}

fn foreign_keys(database: &Connection) -> Result<String> {
    let mut statement = database.prepare("PRAGMA foreign_key_check")?;
    let mut rows = statement.query([])?;
    if let Some(row) = rows.next()? {
        let table: String = row.get(0)?;
        let row_id: Option<i64> = row.get(1)?;
        let parent: String = row.get(2)?;
        bail!(
            "foreign key violation: table {table:?}, row {row_id:?}, parent {parent:?} (first violation)"
        );
    }
    Ok("No foreign key violations.".into())
}

fn assert_query(database: &Connection, assertion: &Assertion) -> Result<String> {
    let mut statement = database
        .prepare(&assertion.sql)
        .context("cannot prepare assertion; use one read-only SELECT statement")?;
    ensure!(statement.readonly(), "assertion must be read-only");
    ensure!(
        statement.column_count() == 1,
        "assertion must return exactly one column"
    );
    let mut rows = statement.query([])?;
    let row = rows
        .next()?
        .context("assertion returned no rows; expected integer 1")?;
    ensure!(
        matches!(row.get_ref(0)?, ValueRef::Integer(1)),
        "assertion did not return integer 1"
    );
    ensure!(
        rows.next()?.is_none(),
        "assertion returned multiple rows; expected exactly one"
    );
    Ok("Assertion returned integer 1.".into())
}

fn read_only_authorizer(context: AuthContext<'_>) -> Authorization {
    match context.action {
        AuthAction::Select | AuthAction::Read { .. } | AuthAction::Recursive => {
            Authorization::Allow
        }
        AuthAction::Function { function_name }
            if !function_name.eq_ignore_ascii_case("load_extension") =>
        {
            Authorization::Allow
        }
        _ => Authorization::Deny,
    }
}

fn with_timeout<T>(
    database: &Connection,
    timeout: Duration,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let started = Instant::now();
    database.progress_handler(1000, Some(move || started.elapsed() >= timeout))?;
    let result = work();
    database.progress_handler(0, None::<fn() -> bool>)?;
    if started.elapsed() >= timeout {
        bail!(
            "query exceeded timeout of {}",
            humantime::format_duration(timeout)
        );
    }
    result
}

fn step(report: &mut Report, name: &str, work: impl FnOnce() -> Result<String>) -> bool {
    let started = Instant::now();
    let (status, detail) = match work() {
        Ok(detail) => (Status::Passed, detail),
        Err(error) => (Status::Failed, format!("{error:#}")),
    };
    report.checks.push(CheckResult {
        name: name.into(),
        status,
        duration_ms: milliseconds(started.elapsed()),
        detail,
    });
    status == Status::Passed
}

fn skip(report: &mut Report, name: &str, detail: &str) {
    report.checks.push(CheckResult {
        name: name.into(),
        status: Status::Skipped,
        duration_ms: 0,
        detail: detail.into(),
    });
}

fn skip_assertions(report: &mut Report, config: &Config, reason: &str) {
    for assertion in &config.checks {
        skip(report, &format!("SQL: {}", assertion.name), reason);
    }
}

fn skip_database_checks(report: &mut Report, config: &Config, reason: &str) {
    skip(report, "SQLite integrity", reason);
    skip(report, "Foreign keys", reason);
    skip_assertions(report, config, reason);
}

fn milliseconds(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}
