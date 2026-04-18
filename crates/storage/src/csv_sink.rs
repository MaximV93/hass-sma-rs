//! CSV file sink — simple append-only storage that doesn't require a
//! running Postgres/TimescaleDB.
//!
//! One file per (slot, YYYY-MM-DD) under the configured root directory:
//! e.g. `/share/hass-sma-rs/zolder-2026-04-19.csv`. Lines are:
//!
//! ```csv
//! time,serial,metric,value
//! 2026-04-19T06:43:12+00:00,2120121246,ac_power,641
//! ```
//!
//! File rotation is implicit via the date in the filename. HA addons
//! typically mount /share as writable, making this a zero-config archive.

use chrono::{DateTime, Datelike, Utc};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::writer::StorageError;

pub struct CsvSink {
    root: PathBuf,
    // One lock serializes writes per sink. Fine for <1k lines/sec.
    write_lock: Mutex<()>,
}

impl CsvSink {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            write_lock: Mutex::new(()),
        }
    }

    fn filename_for(&self, slot: &str, time: &DateTime<Utc>) -> PathBuf {
        self.root.join(format!(
            "{}-{:04}-{:02}-{:02}.csv",
            slot,
            time.year(),
            time.month(),
            time.day()
        ))
    }

    pub async fn append(
        &self,
        time: DateTime<Utc>,
        slot: &str,
        serial: i64,
        metric: &str,
        value: f64,
    ) -> Result<(), StorageError> {
        let path = self.filename_for(slot, &time);
        let _guard = self.write_lock.lock().await;

        // Create dir if needed
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(StorageError::Io)?;
        }

        let is_new = !tokio::fs::try_exists(&path).await.unwrap_or(false);
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(StorageError::Io)?;

        if is_new {
            f.write_all(b"time,serial,metric,value\n")
                .await
                .map_err(StorageError::Io)?;
        }
        let line = format!(
            "{},{},{},{}\n",
            time.to_rfc3339(),
            serial,
            metric,
            value
        );
        f.write_all(line.as_bytes())
            .await
            .map_err(StorageError::Io)?;
        f.flush().await.map_err(StorageError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn appends_header_and_rows() {
        let tmp = TempDir::new().unwrap();
        let sink = CsvSink::new(tmp.path());
        let t = Utc::now();
        sink.append(t, "zolder", 2120121246, "ac_power", 641.0)
            .await
            .unwrap();
        sink.append(t, "zolder", 2120121246, "ac_power", 655.0)
            .await
            .unwrap();

        let path = sink.filename_for("zolder", &t);
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "time,serial,metric,value");
        assert!(lines[1].contains("ac_power,641"));
        assert!(lines[2].contains("ac_power,655"));
    }

    #[tokio::test]
    async fn different_days_different_files() {
        let tmp = TempDir::new().unwrap();
        let sink = CsvSink::new(tmp.path());
        let t1: DateTime<Utc> = "2026-04-18T12:00:00Z".parse().unwrap();
        let t2: DateTime<Utc> = "2026-04-19T12:00:00Z".parse().unwrap();
        sink.append(t1, "x", 1, "a", 1.0).await.unwrap();
        sink.append(t2, "x", 1, "a", 2.0).await.unwrap();
        let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
        let mut n = 0;
        while entries.next_entry().await.unwrap().is_some() {
            n += 1;
        }
        assert_eq!(n, 2);
    }
}
