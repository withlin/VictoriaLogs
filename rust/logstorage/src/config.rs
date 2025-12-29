use std::{path::PathBuf, thread, time::Duration};

use crate::errors::{Result, StorageError};

/// Configuration for a local vlstorage node.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub retention: Duration,
    pub default_parallel_readers: usize,
    pub max_disk_usage_bytes: Option<u64>,
    pub max_disk_usage_percent: Option<u8>,
    pub flush_interval: Duration,
    pub future_retention: Duration,
    pub max_backfill_age: Duration,
    pub storage_data_path: PathBuf,
    pub log_new_streams: bool,
    pub log_ingested_rows: bool,
    pub min_free_disk_space_bytes: u64,
    pub read_only: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        let cpus = thread::available_parallelism()
            .map(|v| v.get())
            .unwrap_or(1);

        Self {
            retention: Duration::from_secs(7 * 24 * 60 * 60),
            default_parallel_readers: cpus.saturating_mul(2),
            max_disk_usage_bytes: None,
            max_disk_usage_percent: None,
            flush_interval: Duration::from_secs(5),
            future_retention: Duration::from_secs(2 * 24 * 60 * 60),
            max_backfill_age: Duration::from_secs(0),
            storage_data_path: PathBuf::from("victoria-logs-data"),
            log_new_streams: false,
            log_ingested_rows: false,
            min_free_disk_space_bytes: 10_000_000,
            read_only: false,
        }
    }
}

impl StorageConfig {
    pub fn validate(&self) -> Result<()> {
        let day = Duration::from_secs(24 * 60 * 60);
        if self.retention < day {
            return Err(StorageError::InvalidConfig(
                "retention must be at least one day".to_string(),
            ));
        }

        if self.max_disk_usage_bytes.is_some() && self.max_disk_usage_percent.is_some() {
            return Err(StorageError::InvalidConfig(
                "max_disk_usage_bytes and max_disk_usage_percent are mutually exclusive".into(),
            ));
        }

        if let Some(percent) = self.max_disk_usage_percent {
            if percent == 0 || percent > 100 {
                return Err(StorageError::InvalidConfig(format!(
                    "max_disk_usage_percent must be between 1 and 100, got {percent}"
                )));
            }
        }

        Ok(())
    }
}
