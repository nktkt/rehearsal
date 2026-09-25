use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::Value;

fn cli(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rehearsal"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn assert_exit(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn demo_check_and_history_work_end_to_end() {
    let directory = tempfile::tempdir().unwrap();
    assert_exit(&cli(directory.path(), &["demo"]), 0);
    let run = cli(
        directory.path(),
        &[
            "check",
            "--config",
            "rehearsal-demo/rehearsal.toml",
            "--json",
        ],
    );
    assert_exit(&run, 0);
    let report: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(report["status"], "passed");
    assert_eq!(report["schema_version"], 1);
    assert!(run.stderr.is_empty());
    let history = cli(
        directory.path(),
        &[
            "history",
            "--config",
            "rehearsal-demo/rehearsal.toml",
            "--json",
        ],
    );
    assert_exit(&history, 0);
    let history: Vec<Value> = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(history.len(), 2);
    assert_exit(&cli(directory.path(), &["demo"]), 2);
}

#[test]
fn failed_checks_are_saved_and_exit_one_while_bad_configuration_exits_two() {
    let directory = tempfile::tempdir().unwrap();
    assert_exit(&cli(directory.path(), &["demo", "--broken"]), 1);
    let missing = cli(directory.path(), &["check", "missing.sqlite", "--json"]);
    assert_exit(&missing, 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&missing.stdout).unwrap()["status"],
        "failed"
    );
    assert!(!directory.path().join("missing.sqlite").exists());
    assert_exit(
        &cli(directory.path(), &["check", "--config", "missing.toml"]),
        2,
    );
    assert_exit(
        &cli(
            directory.path(),
            &["check", "missing.sqlite", "--timeout", "bad"],
        ),
        2,
    );
}

#[test]
fn init_does_not_overwrite_files_and_resolves_the_backup_from_the_working_directory() {
    let directory = tempfile::tempdir().unwrap();
    assert_exit(&cli(directory.path(), &["demo"]), 0);
    fs::create_dir(directory.path().join("config")).unwrap();
    let args = [
        "init",
        "--backup",
        "rehearsal-demo/app.backup.sqlite",
        "--config",
        "config/check.toml",
    ];
    assert_exit(&cli(directory.path(), &args), 0);
    let content = fs::read(directory.path().join("config/check.toml")).unwrap();
    assert_exit(&cli(directory.path(), &args), 2);
    assert_eq!(
        fs::read(directory.path().join("config/check.toml")).unwrap(),
        content
    );
    assert_exit(
        &cli(
            directory.path(),
            &["check", "--config", "config/check.toml"],
        ),
        0,
    );
}

#[test]
fn report_write_errors_preserve_the_backup_and_return_two_with_json() {
    let directory = tempfile::tempdir().unwrap();
    assert_exit(&cli(directory.path(), &["demo"]), 0);
    let backup = "rehearsal-demo/app.backup.sqlite";
    let before = fs::read(directory.path().join(backup)).unwrap();
    let run = cli(
        directory.path(),
        &["check", backup, "--report-dir", backup, "--json"],
    );
    assert_exit(&run, 2);
    assert_eq!(
        serde_json::from_slice::<Value>(&run.stdout).unwrap()["status"],
        "passed"
    );
    assert!(String::from_utf8_lossy(&run.stderr).contains("could not be saved"));
    assert_eq!(fs::read(directory.path().join(backup)).unwrap(), before);
}
