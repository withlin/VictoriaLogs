use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    config::StorageConfig,
    errors::{Result, StorageError},
    model::{LogRow, PartitionSnapshot, Query},
};

const MEM_FLUSH_ROW_THRESHOLD: usize = 1_000;

#[derive(Debug)]
pub enum StorageMode {
    Local(LocalStorage),
    Network(NetStorage),
}

#[derive(Debug)]
pub struct VlStorage {
    mode: StorageMode,
}

impl VlStorage {
    pub fn new_local(config: StorageConfig) -> Result<Self> {
        config.validate()?;

        Ok(Self {
            mode: StorageMode::Local(LocalStorage::new(config)?),
        })
    }

    pub fn new_network(nodes: Vec<NetStorageNode>, disable_compression: bool) -> Result<Self> {
        if nodes.is_empty() {
            return Err(StorageError::InvalidConfig(
                "at least one storage node is required in network mode".into(),
            ));
        }

        Ok(Self {
            mode: StorageMode::Network(NetStorage::new(nodes, disable_compression)),
        })
    }

    pub fn mode(&self) -> &StorageMode {
        &self.mode
    }

    pub fn can_write_data(&self) -> Result<()> {
        match &self.mode {
            StorageMode::Local(local) => local.can_write_data(),
            StorageMode::Network(_) => Ok(()),
        }
    }

    pub fn add_rows(&self, rows: impl IntoIterator<Item = LogRow>) -> Result<()> {
        match &self.mode {
            StorageMode::Local(local) => local.add_rows(rows),
            StorageMode::Network(net) => net.add_rows(rows),
        }
    }

    pub fn run_query(&self, query: &Query) -> Result<Vec<LogRow>> {
        match &self.mode {
            StorageMode::Local(local) => local.run_query(query),
            StorageMode::Network(net) => net.run_query(query),
        }
    }

    pub fn log_new_streams(&self, seconds: u64) -> Result<()> {
        self.require_local()?.log_new_streams(seconds)
    }

    pub fn force_merge(&self, partition_prefix: Option<String>) -> Result<()> {
        self.require_local()?.force_merge(partition_prefix)
    }

    pub fn force_flush(&self) -> Result<()> {
        self.require_local()?.force_flush()
    }

    pub fn partition_attach(&self, name: String) -> Result<()> {
        self.require_local()?.partition_attach(name)
    }

    pub fn partition_detach(&self, name: String) -> Result<()> {
        self.require_local()?.partition_detach(name)
    }

    pub fn partition_list(&self) -> Result<Vec<String>> {
        self.require_local()?.partition_list()
    }

    pub fn partition_snapshot_create(&self, name: String) -> Result<PartitionSnapshot> {
        self.require_local()?.partition_snapshot_create(name)
    }

    pub fn partition_snapshot_list(&self) -> Result<Vec<PartitionSnapshot>> {
        self.require_local()?.partition_snapshot_list()
    }

    fn require_local(&self) -> Result<&LocalStorage> {
        match &self.mode {
            StorageMode::Local(local) => Ok(local),
            StorageMode::Network(_) => Err(StorageError::NotAvailableInMode(
                "operation is only available in local storage mode",
            )),
        }
    }
}

#[derive(Debug)]
pub struct LocalStorage {
    config: StorageConfig,
    state: Mutex<LocalState>,
    wal: Mutex<WalState>,
    retention: RetentionPolicy,
}

#[derive(Debug)]
struct LocalState {
    mem_rows: Vec<LogRow>,
    parts: Vec<Part>,
    partitions: HashSet<String>,
    snapshots: Vec<PartitionSnapshot>,
    log_new_streams_until: Option<Instant>,
    last_force_merge: Option<ForceMerge>,
    last_force_flush: Option<Instant>,
}

#[derive(Debug)]
struct WalState {
    writer: BufWriter<File>,
    last_flush: Instant,
    path: PathBuf,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Part {
    id: String,
    partition: String,
    path: PathBuf,
    index_path: PathBuf,
    rows: usize,
    min_ts: i64,
    max_ts: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct PartIndex {
    min_ts: i64,
    max_ts: i64,
    rows: usize,
    field_counts: HashMap<String, HashMap<String, u64>>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct ForceMerge {
    partition_prefix: Option<String>,
    started_at: Instant,
}

impl LocalStorage {
    fn new(config: StorageConfig) -> Result<Self> {
        let retention = RetentionPolicy::from_config(&config);
        fs::create_dir_all(&config.storage_data_path)
            .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let wal_path = config.storage_data_path.join("wal.jsonl");
        let wal_writer = open_wal_for_append(&wal_path)?;

        let mut parts = load_parts_from_disk(&config.storage_data_path)?;
        retention.apply_in_place_parts(&mut parts);

        let mut mem_rows = load_rows_from_wal(&wal_path, &retention)?;
        retention.apply_in_place(&mut mem_rows);

        let partitions = collect_partitions(&parts, &mem_rows);

        Ok(Self {
            config,
            state: Mutex::new(LocalState {
                mem_rows,
                parts,
                partitions,
                snapshots: Vec::new(),
                log_new_streams_until: None,
                last_force_merge: None,
                last_force_flush: None,
            }),
            wal: Mutex::new(WalState {
                writer: wal_writer,
                last_flush: Instant::now(),
                path: wal_path,
            }),
            retention,
        })
    }

    fn can_write_data(&self) -> Result<()> {
        if self.config.read_only {
            let path = self.config.storage_data_path.display().to_string();
            return Err(StorageError::ReadOnly(format!(
                "cannot add rows into storage in read-only mode; storage path: {path}"
            )));
        }

        Ok(())
    }

    fn add_rows(&self, rows: impl IntoIterator<Item = LogRow>) -> Result<()> {
        self.can_write_data()?;

        let now = current_time_nanos();
        let mut wal = self.wal.lock().expect("wal lock poisoned");
        let mut state = self.state.lock().expect("local state lock poisoned");

        for row in rows {
            self.retention.validate_row(&row, now)?;

            let serialized = serde_json::to_string(&row).map_err(|e| {
                StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("serialize row: {e}"),
                ))
            })?;

            wal.writer
                .write_all(serialized.as_bytes())
                .and_then(|_| wal.writer.write_all(b"\n"))?;

            state.partitions.insert(partition_name(row.timestamp));
            state.mem_rows.push(row);
        }

        if state.mem_rows.len() >= MEM_FLUSH_ROW_THRESHOLD {
            self.flush_mem_to_parts_locked(&mut state, &mut wal)?;
        } else if wal.last_flush.elapsed() >= self.config.flush_interval {
            wal.writer.flush()?;
            wal.last_flush = Instant::now();
        }

        Ok(())
    }

    fn flush_mem_to_parts_locked(&self, state: &mut LocalState, wal: &mut WalState) -> Result<()> {
        if state.mem_rows.is_empty() {
            return Ok(());
        }

        let mut by_partition: HashMap<String, Vec<LogRow>> = HashMap::new();
        for row in state.mem_rows.drain(..) {
            by_partition
                .entry(partition_name(row.timestamp))
                .or_default()
                .push(row);
        }

        for (partition, rows) in by_partition {
            let part_id = format!("part-{}", current_time_nanos());
            let (part, index) =
                write_part(&self.config.storage_data_path, &partition, &part_id, rows)?;
            state.parts.push(part);

            // Write index file
            let idx_path = part_index_path(&self.config.storage_data_path, &partition, &part_id);
            write_index(&idx_path, &index)?;
        }

        wal.writer.flush()?;
        wal.last_flush = Instant::now();
        self.retention.apply_in_place_parts(&mut state.parts);
        Ok(())
    }

    fn force_flush(&self) -> Result<()> {
        let mut wal = self.wal.lock().expect("wal lock poisoned");
        let mut state = self.state.lock().expect("local state lock poisoned");

        self.flush_mem_to_parts_locked(&mut state, &mut wal)?;

        // Reset WAL to avoid unbounded growth
        wal.writer.flush()?;
        wal.writer = open_wal_truncate(&wal.path)?;
        wal.last_flush = Instant::now();
        state.last_force_flush = Some(Instant::now());
        Ok(())
    }

    fn run_query(&self, query: &Query) -> Result<Vec<LogRow>> {
        let mut rows: Vec<LogRow> = {
            let state = self.state.lock().expect("local state lock poisoned");
            state
                .mem_rows
                .iter()
                .filter(|row| row.matches(query))
                .cloned()
                .collect()
        };

        // Read parts from disk
        let parts = {
            let state = self.state.lock().expect("local state lock poisoned");
            state.parts.clone()
        };
        let (start_opt, end_opt) = query_time_range(query);
        for part in parts {
            if !part_overlaps(&part, start_opt, end_opt) {
                continue;
            }
            let f = File::open(&part.path).map_err(StorageError::Io)?;
            let reader = BufReader::new(f);
            for line in reader.lines() {
                let line = line.map_err(StorageError::Io)?;
                if line.is_empty() {
                    continue;
                }
                let row: LogRow = serde_json::from_str(&line).map_err(|e| {
                    StorageError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("parse row: {e}"),
                    ))
                })?;
                if row.matches(query) {
                    rows.push(row);
                }
            }
        }

        rows.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        if let Some(limit) = query.limit {
            if rows.len() > limit {
                rows.truncate(limit);
            }
        }

        Ok(rows)
    }

    fn log_new_streams(&self, seconds: u64) -> Result<()> {
        let mut state = self.state.lock().expect("local state lock poisoned");
        state.log_new_streams_until = Some(Instant::now() + Duration::from_secs(seconds));
        Ok(())
    }

    fn force_merge(&self, partition_prefix: Option<String>) -> Result<()> {
        let mut state = self.state.lock().expect("local state lock poisoned");
        let mut groups: HashMap<String, Vec<Part>> = HashMap::new();
        for part in state.parts.drain(..) {
            if let Some(prefix) = &partition_prefix {
                if !part.partition.starts_with(prefix) {
                    groups.entry(part.partition.clone()).or_default().push(part);
                    continue;
                }
            }
            groups.entry(part.partition.clone()).or_default().push(part);
        }

        for (partition, parts) in groups {
            if parts.len() <= 1 {
                state.parts.extend(parts);
                continue;
            }
            let part_id = format!("merge-{}", current_time_nanos());
            let merged = merge_parts(&self.config.storage_data_path, &partition, &part_id, &parts)?;
            state.parts.push(merged);
            for p in parts {
                let _ = fs::remove_file(&p.path);
                let _ = fs::remove_file(&p.index_path);
            }
        }
        state.last_force_merge = Some(ForceMerge {
            partition_prefix,
            started_at: Instant::now(),
        });
        Ok(())
    }

    fn partition_attach(&self, name: String) -> Result<()> {
        let mut state = self.state.lock().expect("local state lock poisoned");
        state.partitions.insert(name);
        Ok(())
    }

    fn partition_detach(&self, name: String) -> Result<()> {
        let mut state = self.state.lock().expect("local state lock poisoned");
        state.partitions.remove(&name);
        state.snapshots.retain(|s| s.partition != name);
        state.parts.retain(|p| p.partition != name);
        Ok(())
    }

    fn partition_list(&self) -> Result<Vec<String>> {
        let state = self.state.lock().expect("local state lock poisoned");
        let mut names: Vec<String> = state.partitions.iter().cloned().collect();
        names.sort();
        Ok(names)
    }

    fn partition_snapshot_create(&self, name: String) -> Result<PartitionSnapshot> {
        let mut state = self.state.lock().expect("local state lock poisoned");
        state.partitions.insert(name.clone());

        let path = make_snapshot_path(&self.config.storage_data_path, &name);
        let snapshot = PartitionSnapshot::new(name, path);

        state.snapshots.push(snapshot.clone());
        Ok(snapshot)
    }

    fn partition_snapshot_list(&self) -> Result<Vec<PartitionSnapshot>> {
        let state = self.state.lock().expect("local state lock poisoned");
        Ok(state.snapshots.clone())
    }
}

fn make_snapshot_path(base: &PathBuf, partition: &str) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut path = base.clone();
    path.push("snapshots");
    path.push(format!("{partition}-{timestamp}.tar"));
    path.to_string_lossy().to_string()
}

#[derive(Debug, Clone)]
struct RetentionPolicy {
    retention: Duration,
    future_retention: Duration,
    max_backfill_age: Duration,
}

impl RetentionPolicy {
    fn from_config(cfg: &StorageConfig) -> Self {
        Self {
            retention: cfg.retention,
            future_retention: cfg.future_retention,
            max_backfill_age: cfg.max_backfill_age,
        }
    }

    fn validate_row(&self, row: &LogRow, now: i64) -> Result<()> {
        let too_old = now - row.timestamp > self.retention.as_nanos() as i64;
        if too_old {
            return Err(StorageError::InvalidConfig(
                "row timestamp is older than retention".into(),
            ));
        }

        if self.max_backfill_age.as_nanos() > 0 {
            let max_age = self.max_backfill_age.as_nanos() as i64;
            if now - row.timestamp > max_age {
                return Err(StorageError::InvalidConfig(
                    "row timestamp is older than max_backfill_age".into(),
                ));
            }
        }

        let too_future = row.timestamp - now > self.future_retention.as_nanos() as i64;
        if too_future {
            return Err(StorageError::InvalidConfig(
                "row timestamp is too far in the future".into(),
            ));
        }
        Ok(())
    }

    fn apply_in_place(&self, rows: &mut Vec<LogRow>) {
        let now = current_time_nanos();
        rows.retain(|r| self.validate_row(r, now).is_ok());
    }

    fn apply_in_place_parts(&self, parts: &mut Vec<Part>) {
        let now = current_time_nanos();
        parts.retain(|p| now - p.max_ts <= self.retention.as_nanos() as i64);
    }
}

fn current_time_nanos() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64
}

fn partition_name(timestamp: i64) -> String {
    let day = timestamp / 1_000_000_000 / 86_400;
    format!("p{day}")
}

fn part_path(base: &Path, partition: &str, part_id: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    p.push(partition);
    p.push("parts");
    fs::create_dir_all(&p).ok();
    p.push(format!("{part_id}.jsonl"));
    p
}

fn part_index_path(base: &Path, partition: &str, part_id: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    p.push(partition);
    p.push("indexes");
    fs::create_dir_all(&p).ok();
    p.push(format!("{part_id}.json"));
    p
}

fn write_part(
    base: &Path,
    partition: &str,
    part_id: &str,
    rows: Vec<LogRow>,
) -> Result<(Part, PartIndex)> {
    let path = part_path(base, partition, part_id);
    let mut writer = BufWriter::new(File::create(&path).map_err(StorageError::Io)?);

    let mut min_ts = i64::MAX;
    let mut max_ts = i64::MIN;
    let mut field_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();
    for row in &rows {
        min_ts = min_ts.min(row.timestamp);
        max_ts = max_ts.max(row.timestamp);
        for f in &row.fields {
            *field_counts
                .entry(f.name.clone())
                .or_default()
                .entry(f.value.clone())
                .or_default() += 1;
        }
        let line = serde_json::to_string(row).map_err(|e| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("serialize row: {e}"),
            ))
        })?;
        writer
            .write_all(line.as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .map_err(StorageError::Io)?;
    }
    writer.flush().map_err(StorageError::Io)?;

    let index = PartIndex {
        min_ts,
        max_ts,
        rows: rows.len(),
        field_counts,
    };
    let index_path = part_index_path(base, partition, part_id);
    Ok((
        Part {
            id: part_id.to_string(),
            partition: partition.to_string(),
            path,
            index_path,
            rows: rows.len(),
            min_ts,
            max_ts,
        },
        index,
    ))
}

fn write_index(path: &Path, index: &PartIndex) -> Result<()> {
    let file = File::create(path).map_err(StorageError::Io)?;
    serde_json::to_writer_pretty(BufWriter::new(file), index).map_err(|e| {
        StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("write index: {e}"),
        ))
    })
}

fn load_parts_from_disk(base: &Path) -> Result<Vec<Part>> {
    let mut parts = Vec::new();
    if !base.exists() {
        return Ok(parts);
    }

    for entry in fs::read_dir(base).map_err(StorageError::Io)? {
        let entry = entry.map_err(StorageError::Io)?;
        if !entry.file_type().map_err(StorageError::Io)?.is_dir() {
            continue;
        }
        let partition = entry
            .file_name()
            .into_string()
            .unwrap_or_else(|_| "unknown".into());
        let parts_dir = entry.path().join("parts");
        if !parts_dir.exists() {
            continue;
        }
        for part_entry in fs::read_dir(parts_dir).map_err(StorageError::Io)? {
            let part_entry = part_entry.map_err(StorageError::Io)?;
            let path = part_entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let part_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            let index_path = part_index_path(base, &partition, &part_id);
            let (min_ts, max_ts, rows) = load_part_meta(&path, &index_path)?;
            parts.push(Part {
                id: part_id,
                partition: partition.clone(),
                path,
                index_path,
                rows,
                min_ts,
                max_ts,
            });
        }
    }

    Ok(parts)
}

fn load_part_meta(path: &Path, index_path: &Path) -> Result<(i64, i64, usize)> {
    if index_path.exists() {
        let file = File::open(index_path).map_err(StorageError::Io)?;
        let idx: PartIndex = serde_json::from_reader(BufReader::new(file)).map_err(|e| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("read index: {e}"),
            ))
        })?;
        return Ok((idx.min_ts, idx.max_ts, idx.rows));
    }

    // Fallback: scan part
    let file = File::open(path).map_err(StorageError::Io)?;
    let reader = BufReader::new(file);
    let mut min_ts = i64::MAX;
    let mut max_ts = i64::MIN;
    let mut rows = 0usize;
    for line in reader.lines() {
        let line = line.map_err(StorageError::Io)?;
        if line.is_empty() {
            continue;
        }
        let row: LogRow = serde_json::from_str(&line).map_err(|e| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("parse row: {e}"),
            ))
        })?;
        min_ts = min_ts.min(row.timestamp);
        max_ts = max_ts.max(row.timestamp);
        rows += 1;
    }
    Ok((min_ts, max_ts, rows))
}

fn merge_parts(base: &Path, partition: &str, part_id: &str, parts: &[Part]) -> Result<Part> {
    let mut merged_rows = Vec::new();
    for part in parts {
        let file = File::open(&part.path).map_err(StorageError::Io)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line.map_err(StorageError::Io)?;
            if line.is_empty() {
                continue;
            }
            let row: LogRow = serde_json::from_str(&line).map_err(|e| {
                StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("parse row: {e}"),
                ))
            })?;
            merged_rows.push(row);
        }
    }

    let (part, index) = write_part(base, partition, part_id, merged_rows)?;
    write_index(&part.index_path, &index)?;
    Ok(part)
}

fn load_rows_from_wal(path: &PathBuf, retention: &RetentionPolicy) -> Result<Vec<LogRow>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(Vec::new()),
    };

    let reader = BufReader::new(file);
    let mut rows = Vec::new();
    for line in reader.lines() {
        match line {
            Ok(line) if !line.is_empty() => match serde_json::from_str::<LogRow>(&line) {
                Ok(row) => {
                    if retention.validate_row(&row, current_time_nanos()).is_ok() {
                        rows.push(row);
                    }
                }
                Err(err) => {
                    eprintln!("failed to parse wal line: {err}");
                }
            },
            _ => {}
        }
    }
    Ok(rows)
}

fn open_wal_for_append(path: &PathBuf) -> Result<BufWriter<File>> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(StorageError::Io)?;
    Ok(BufWriter::new(file))
}

fn open_wal_truncate(path: &PathBuf) -> Result<BufWriter<File>> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(StorageError::Io)?;
    Ok(BufWriter::new(file))
}

fn collect_partitions(parts: &[Part], mem_rows: &[LogRow]) -> HashSet<String> {
    let mut set = HashSet::new();
    for p in parts {
        set.insert(p.partition.clone());
    }
    for r in mem_rows {
        set.insert(partition_name(r.timestamp));
    }
    set
}

fn part_overlaps(part: &Part, start: Option<i64>, end: Option<i64>) -> bool {
    if let Some(start) = start {
        if part.max_ts < start {
            return false;
        }
    }
    if let Some(end) = end {
        if part.min_ts > end {
            return false;
        }
    }
    true
}

fn query_time_range(q: &Query) -> (Option<i64>, Option<i64>) {
    (q.start, q.end)
}

#[derive(Debug, Clone)]
pub struct NetStorageNode {
    pub address: String,
    pub tls: bool,
}

#[derive(Debug)]
pub struct NetStorage {
    #[allow(dead_code)]
    nodes: Vec<NetStorageNode>,
    disable_compression: bool,
}

impl NetStorage {
    fn new(nodes: Vec<NetStorageNode>, disable_compression: bool) -> Self {
        Self {
            nodes,
            disable_compression,
        }
    }

    fn add_rows(&self, _rows: impl IntoIterator<Item = LogRow>) -> Result<()> {
        let _ = self.disable_compression;
        Err(StorageError::NotImplemented(
            "network ingestion is not implemented in the Rust port yet",
        ))
    }

    fn run_query(&self, _query: &Query) -> Result<Vec<LogRow>> {
        Err(StorageError::NotImplemented(
            "network querying is not implemented in the Rust port yet",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LogField;
    use std::{env, fs};

    fn temp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("logstorage-test-{name}-{}", current_time_nanos()));
        p
    }

    #[test]
    fn add_and_query_rows() {
        let mut cfg = StorageConfig::default();
        cfg.storage_data_path = temp_path("add-query");
        let storage = VlStorage::new_local(cfg.clone()).unwrap();
        let base = current_time_nanos();
        let rows = vec![
            LogRow::new(
                base,
                vec![
                    LogField::new("message", "first"),
                    LogField::new("level", "info"),
                ],
            ),
            LogRow::new(
                base + 1_000,
                vec![
                    LogField::new("message", "second"),
                    LogField::new("level", "warn"),
                ],
            ),
        ];
        storage.add_rows(rows).unwrap();

        let query = Query {
            start: Some(base + 500),
            end: None,
            limit: None,
            filters: vec![crate::model::FieldFilter::Equals {
                name: "level".into(),
                value: "warn".into(),
            }],
        };

        let results = storage.run_query(&query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp, base + 1_000);

        let _ = fs::remove_dir_all(&cfg.storage_data_path);
    }

    #[test]
    fn persist_to_parts_and_recover() {
        let mut cfg = StorageConfig::default();
        cfg.storage_data_path = temp_path("recover");
        let base = current_time_nanos();
        {
            let storage = VlStorage::new_local(cfg.clone()).unwrap();
            let rows = vec![LogRow::new(
                base,
                vec![LogField::new("level", "info"), LogField::new("msg", "x")],
            )];
            storage.add_rows(rows).unwrap();
            storage.force_flush().unwrap();
        }
        let storage = VlStorage::new_local(cfg.clone()).unwrap();
        let results = storage.run_query(&Query::default()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp, base);
        let _ = fs::remove_dir_all(&cfg.storage_data_path);
    }

    #[test]
    fn partition_lifecycle() {
        let mut cfg = StorageConfig::default();
        cfg.storage_data_path = temp_path("partition");
        let storage = VlStorage::new_local(cfg.clone()).unwrap();
        storage.partition_attach("p1".into()).unwrap();
        storage.partition_attach("p2".into()).unwrap();

        let names = storage.partition_list().unwrap();
        assert_eq!(names, vec!["p1".to_string(), "p2".to_string()]);

        let snapshot = storage.partition_snapshot_create("p1".into()).unwrap();
        assert!(snapshot.path.contains("p1"));

        let snapshots = storage.partition_snapshot_list().unwrap();
        assert_eq!(snapshots.len(), 1);

        storage.partition_detach("p1".into()).unwrap();
        let names = storage.partition_list().unwrap();
        assert_eq!(names, vec!["p2".to_string()]);

        let _ = fs::remove_dir_all(&cfg.storage_data_path);
    }
}
