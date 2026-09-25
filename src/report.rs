use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Failed,
    Skipped,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Passed => "PASS",
            Self::Failed => "FAIL",
            Self::Skipped => "SKIP",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub status: Status,
    pub duration_ms: u64,
    pub detail: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub tool_version: String,
    pub sqlite_version: String,
    pub name: String,
    pub source: PathBuf,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub status: Status,
    pub restored_bytes: Option<u64>,
    pub backup_modified_at: Option<DateTime<Utc>>,
    pub checks: Vec<CheckResult>,
}

impl Report {
    /// Write a complete report atomically; existing files are never overwritten.
    pub fn save(&self, directory: &Path) -> Result<PathBuf> {
        fs::create_dir_all(directory)
            .with_context(|| format!("cannot create report directory {}", directory.display()))?;
        let mut temporary =
            NamedTempFile::new_in(directory).context("cannot create report file")?;
        serde_json::to_writer_pretty(&mut temporary, self)?;
        writeln!(temporary)?;
        temporary.as_file().sync_all()?;
        let unique = temporary
            .path()
            .file_name()
            .context("report file has no name")?
            .to_string_lossy();
        let path = directory.join(format!(
            "run-{}-{unique}.json",
            self.started_at.format("%Y%m%dT%H%M%S%.9fZ")
        ));
        temporary
            .persist_noclobber(&path)
            .with_context(|| format!("cannot save report {}", path.display()))?;
        Ok(path)
    }

    pub fn render(&self) -> String {
        let mut lines = vec![
            format!("Rehearsal · {}", self.name),
            format!("Backup    {}", self.source.display()),
            String::new(),
        ];
        for check in &self.checks {
            lines.push(format!(
                "  {:4}  {} ({} ms)",
                check.status.label(),
                check.name,
                check.duration_ms
            ));
            for line in check.detail.lines() {
                lines.push(format!("        {line}"));
            }
        }
        lines.push(String::new());
        lines.push(format!(
            "{} · {} ms · {}",
            self.status.label(),
            self.duration_ms,
            self.started_at.to_rfc3339()
        ));
        if self.status == Status::Passed {
            lines.push(
                "The backup passed the configured checks. Application startup was not tested."
                    .into(),
            );
        }
        lines.join("\n")
    }
}

pub fn history(directory: &Path, limit: usize) -> Result<Vec<Report>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("cannot read history {}", directory.display()));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
            && entry.file_name().to_string_lossy().starts_with("run-")
        {
            paths.push(path);
        }
    }
    paths.sort_unstable_by(|left, right| right.cmp(left));
    paths
        .into_iter()
        .take(limit)
        .map(|path| {
            let report: Report = serde_json::from_reader(fs::File::open(&path)?)
                .with_context(|| format!("invalid report {}", path.display()))?;
            ensure!(
                report.schema_version == 1,
                "unsupported report schema in {}",
                path.display()
            );
            Ok(report)
        })
        .collect()
}
