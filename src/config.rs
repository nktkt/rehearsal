use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// All relative paths in a configuration are relative to that configuration file.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_name")]
    pub name: String,
    pub backup: PathBuf,
    #[serde(default = "default_report_dir")]
    pub report_dir: PathBuf,
    /// File modification age, not the age of the newest database record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout: String,
    #[serde(default)]
    pub checks: Vec<Assertion>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub name: String,
    /// A single read-only query returning exactly one row, one column, integer 1.
    pub sql: String,
}

fn default_name() -> String {
    "SQLite backup".into()
}
fn default_report_dir() -> PathBuf {
    ".rehearsal/reports".into()
}
fn default_timeout() -> String {
    "30s".into()
}

impl Config {
    pub fn for_backup(backup: PathBuf) -> Self {
        Self {
            name: default_name(),
            backup,
            report_dir: default_report_dir(),
            max_age: None,
            timeout: default_timeout(),
            checks: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "cannot read {}; use `rehearsal init --backup PATH` or `rehearsal demo` to start",
                path.display()
            )
        })?;
        let mut config: Self = toml::from_str(&content)
            .with_context(|| format!("invalid configuration: {}", path.display()))?;
        config.validate()?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        config.backup = base.join(&config.backup);
        config.report_dir = base.join(&config.report_dir);
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "name cannot be empty");
        ensure!(
            !self.backup.as_os_str().is_empty(),
            "backup path cannot be empty"
        );
        ensure!(
            !self.report_dir.as_os_str().is_empty(),
            "report_dir cannot be empty"
        );
        self.query_timeout()?;
        self.max_backup_age()?;
        let mut names = HashSet::new();
        for check in &self.checks {
            ensure!(!check.name.trim().is_empty(), "check name cannot be empty");
            ensure!(
                !check.sql.trim().is_empty(),
                "SQL for check {:?} cannot be empty",
                check.name
            );
            ensure!(
                names.insert(&check.name),
                "duplicate check name: {}",
                check.name
            );
        }
        Ok(())
    }

    pub fn query_timeout(&self) -> Result<Duration> {
        parse_duration("timeout", &self.timeout)
    }

    pub fn max_backup_age(&self) -> Result<Option<Duration>> {
        self.max_age
            .as_deref()
            .map(|age| parse_duration("max_age", age))
            .transpose()
    }
}

fn parse_duration(name: &str, value: &str) -> Result<Duration> {
    let duration = humantime::parse_duration(value)
        .with_context(|| format!("invalid {name}: {value:?}; use a duration such as 30s or 24h"))?;
    ensure!(!duration.is_zero(), "{name} must be greater than zero");
    Ok(duration)
}
