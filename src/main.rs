use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use rehearsal::{
    config::{Assertion, Config},
    report::{self, Status},
    runner,
};

#[derive(Parser)]
#[command(
    name = "rehearsal",
    version,
    about = "Rehearse your SQLite recovery.",
    long_about = "Restore a completed SQLite backup into a temporary directory, verify integrity and application assertions, and keep a JSON report."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Restore and verify a backup, then save a report.
    Check(CheckArgs),
    /// Create a configuration without overwriting an existing file.
    Init {
        /// Completed, standalone SQLite backup (relative to the current directory).
        #[arg(long)]
        backup: PathBuf,
        #[arg(short, long, default_value = "rehearsal.toml")]
        config: PathBuf,
    },
    /// Show previous runs, newest first.
    History {
        #[arg(short, long, conflicts_with = "report_dir")]
        config: Option<PathBuf>,
        #[arg(long)]
        report_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Create a sample backup and run your first restore drill.
    Demo {
        /// A new directory; an existing directory is never overwritten.
        #[arg(long, default_value = "rehearsal-demo")]
        dir: PathBuf,
        /// Include a foreign key violation to demonstrate a failed drill.
        #[arg(long)]
        broken: bool,
    },
}

#[derive(Args)]
struct CheckArgs {
    /// Backup file to check without a configuration.
    #[arg(conflicts_with = "config")]
    backup: Option<PathBuf>,
    /// Configuration file (defaults to rehearsal.toml when no backup is supplied).
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Directory for reports, relative to the current directory.
    #[arg(long)]
    report_dir: Option<PathBuf>,
    /// Maximum backup file modification age, for example 24h.
    #[arg(long)]
    max_age: Option<String>,
    /// Time budget per SQLite query, for example 30s.
    #[arg(long)]
    timeout: Option<String>,
    /// Emit one JSON report on stdout; diagnostics go to stderr.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("rehearsal: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn execute(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Check(args) => {
            let mut config = match args.backup {
                Some(backup) => Config::for_backup(backup),
                None => Config::load(
                    args.config
                        .as_deref()
                        .unwrap_or_else(|| Path::new("rehearsal.toml")),
                )?,
            };
            if let Some(directory) = args.report_dir {
                config.report_dir = directory;
            }
            if let Some(max_age) = args.max_age {
                config.max_age = Some(max_age);
            }
            if let Some(timeout) = args.timeout {
                config.timeout = timeout;
            }
            run_check(&config, args.json)
        }
        Command::Init {
            backup,
            config: path,
        } => {
            // Store an absolute source so moving the config into a subdirectory
            // cannot silently change which backup is checked.
            let mut config = Config::for_backup(std::path::absolute(backup)?);
            config.max_age = Some("24h".into());
            write_config(&path, &config)?;
            println!(
                "Created {}\nNext: rehearsal check --config {}",
                path.display(),
                path.display()
            );
            Ok(0)
        }
        Command::History {
            config,
            report_dir,
            limit,
            json,
        } => {
            let directory = match (config, report_dir) {
                (_, Some(directory)) => directory,
                (Some(path), _) => Config::load(&path)?.report_dir,
                (None, None) => {
                    let path = Path::new("rehearsal.toml");
                    if path.try_exists()? {
                        Config::load(path)?.report_dir
                    } else {
                        Config::for_backup(PathBuf::new()).report_dir
                    }
                }
            };
            let reports = report::history(&directory, limit as usize)?;
            if json {
                let mut output = io::stdout().lock();
                serde_json::to_writer_pretty(&mut output, &reports)?;
                writeln!(output)?;
            } else if reports.is_empty() {
                println!(
                    "No runs in {}. Run `rehearsal check` to record a drill.",
                    directory.display()
                );
            } else {
                println!("Rehearsal · recent runs\n");
                for report in reports {
                    println!(
                        "{}  {:4}  {:>6} ms  {}",
                        report.started_at.format("%Y-%m-%d %H:%M:%S UTC"),
                        report.status.label(),
                        report.duration_ms,
                        report.name
                    );
                }
            }
            Ok(0)
        }
        Command::Demo { dir, broken } => {
            create_demo(&dir, broken)?;
            println!(
                "Created sample backup and configuration in {}\n",
                dir.display()
            );
            let path = dir.join("rehearsal.toml");
            let code = run_check(&Config::load(&path)?, false)?;
            println!("\nRun again: rehearsal check --config {}", path.display());
            Ok(code)
        }
    }
}

fn run_check(config: &Config, json: bool) -> Result<u8> {
    let report = runner::run(config)?;
    let saved = report.save(&config.report_dir);
    // Even if persistence fails, JSON callers still receive the completed report.
    let mut output = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut output, &report)?;
        writeln!(output)?;
    } else {
        writeln!(output, "{}", report.render())?;
    }
    output.flush()?;
    let path = saved.context("drill finished, but its report could not be saved")?;
    if !json {
        writeln!(output, "Report    {}", path.display())?;
    }
    Ok(if report.status == Status::Passed {
        0
    } else {
        1
    })
}

fn write_config(path: &Path, config: &Config) -> Result<()> {
    config.validate()?;
    let content = toml::to_string_pretty(config)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "cannot create {}; existing files are never overwritten",
                path.display()
            )
        })?;
    writeln!(
        file,
        "# Rehearsal: paths below are relative to this file.\n# SQL checks must return one row, one column, integer 1.\n\n{content}"
    )?;
    Ok(())
}

fn create_demo(directory: &Path, broken: bool) -> Result<()> {
    fs::create_dir(directory).with_context(|| {
        format!(
            "cannot create demo {}; choose a new directory with --dir",
            directory.display()
        )
    })?;
    let database = rusqlite::Connection::open(directory.join("app.backup.sqlite"))?;
    database.execute_batch("PRAGMA foreign_keys = OFF;
        CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE notes (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id), body TEXT NOT NULL, created_at TEXT NOT NULL);
        INSERT INTO users VALUES (1, 'Ada');
        INSERT INTO notes VALUES (1, 1, 'Backups should be rehearsed.', datetime('now'));
        INSERT INTO notes VALUES (2, 1, 'Small tools, clear outcomes.', datetime('now'));")?;
    if broken {
        database.execute(
            "INSERT INTO notes VALUES (3, 999, 'An orphaned note.', datetime('now'))",
            [],
        )?;
    }
    database.close().map_err(|(_, error)| error)?;
    let mut config = Config::for_backup("app.backup.sqlite".into());
    config.name = "Notes demo".into();
    config.max_age = Some("24h".into());
    config.checks = vec![
        Assertion {
            name: "Users exist".into(),
            sql: "SELECT EXISTS(SELECT 1 FROM users)".into(),
        },
        Assertion {
            name: "At least two notes survived".into(),
            sql: "SELECT COUNT(*) >= 2 FROM notes".into(),
        },
        Assertion {
            name: "Recent note exists".into(),
            sql: "SELECT COALESCE(MAX(created_at) >= datetime('now', '-1 day'), 0) FROM notes"
                .into(),
        },
    ];
    write_config(&directory.join("rehearsal.toml"), &config)
}
