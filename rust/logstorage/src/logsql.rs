use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    model::{LogRow, Query},
    storage::VlStorage,
};

/// Shared value + hit count representation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValueWithHits {
    pub value: String,
    pub hits: u64,
}

impl ValueWithHits {
    pub fn new(value: impl Into<String>, hits: u64) -> Self {
        Self {
            value: value.into(),
            hits,
        }
    }
}

/// Request for computing facets.
#[derive(Debug, Clone)]
pub struct FacetRequest {
    pub query: Query,
    pub field: String,
    pub limit: Option<usize>,
}

/// Request for computing hits over time.
#[derive(Debug, Clone)]
pub struct HitsRequest {
    pub query: Query,
    /// Step size in nanoseconds.
    pub step: i64,
    /// Offset in nanoseconds.
    pub offset: i64,
    /// Optional grouping fields. If empty, a single series is returned.
    pub fields: Vec<String>,
    pub limit: Option<usize>,
}

/// Point in a time series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HitPoint {
    pub timestamp: i64,
    pub hits: u64,
}

/// Hits time series for a particular group key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HitSeries {
    pub key: String,
    pub points: Vec<HitPoint>,
    pub total: u64,
}

/// LogSQL-like API backed by the in-memory storage.
pub struct LogSqlEngine<'a> {
    storage: &'a VlStorage,
}

impl<'a> LogSqlEngine<'a> {
    pub fn new(storage: &'a VlStorage) -> Self {
        Self { storage }
    }

    /// Returns min/max timestamp for the given query if any rows exist.
    pub fn query_time_range(&self, query: &Query) -> crate::errors::Result<Option<(i64, i64)>> {
        let rows = self.storage.run_query(query)?;
        let Some(first) = rows.first() else {
            return Ok(None);
        };
        let mut min_ts = first.timestamp;
        let mut max_ts = first.timestamp;
        for row in rows.iter().skip(1) {
            if row.timestamp < min_ts {
                min_ts = row.timestamp;
            }
            if row.timestamp > max_ts {
                max_ts = row.timestamp;
            }
        }
        Ok(Some((min_ts, max_ts)))
    }

    /// Returns unique field names with hit counts.
    ///
    /// Stream fields (prefixed with `_stream`) are excluded; use
    /// [`stream_field_names`](Self::stream_field_names) instead.
    pub fn field_names(&self, query: &Query) -> crate::errors::Result<Vec<ValueWithHits>> {
        self.collect_field_names(query, false)
    }

    /// Returns unique stream field names with hit counts.
    pub fn stream_field_names(&self, query: &Query) -> crate::errors::Result<Vec<ValueWithHits>> {
        self.collect_field_names(query, true)
    }

    fn collect_field_names(
        &self,
        query: &Query,
        only_stream: bool,
    ) -> crate::errors::Result<Vec<ValueWithHits>> {
        let rows = self.storage.run_query(query)?;
        let mut counts: HashMap<String, u64> = HashMap::new();
        for row in rows {
            for field in row.fields {
                let is_stream = is_stream_field(&field.name);
                if only_stream != is_stream {
                    continue;
                }
                *counts.entry(field.name).or_default() += 1;
            }
        }
        Ok(to_sorted_hits(counts, None))
    }

    /// Returns unique values and hit counts for the given field name.
    pub fn field_values(
        &self,
        query: &Query,
        field: &str,
        limit: Option<usize>,
    ) -> crate::errors::Result<Vec<ValueWithHits>> {
        self.collect_values(query, field, limit, false)
    }

    /// Returns unique values and hit counts for the given stream field name.
    pub fn stream_field_values(
        &self,
        query: &Query,
        field: &str,
        limit: Option<usize>,
    ) -> crate::errors::Result<Vec<ValueWithHits>> {
        self.collect_values(query, field, limit, true)
    }

    fn collect_values(
        &self,
        query: &Query,
        field: &str,
        limit: Option<usize>,
        require_stream: bool,
    ) -> crate::errors::Result<Vec<ValueWithHits>> {
        let rows = self.storage.run_query(query)?;
        let mut counts: HashMap<String, u64> = HashMap::new();
        for row in rows {
            for f in row.fields {
                if f.name != field {
                    continue;
                }
                if require_stream && !is_stream_field(&f.name) {
                    continue;
                }
                *counts.entry(f.value).or_default() += 1;
            }
        }
        Ok(to_sorted_hits(counts, limit))
    }

    /// Returns a time series of hits grouped by optional field set.
    pub fn hits(&self, req: HitsRequest) -> crate::errors::Result<Vec<HitSeries>> {
        let rows = self.storage.run_query(&req.query)?;
        let mut series: HashMap<String, HitSeries> = HashMap::new();

        for row in rows {
            let ts = bucket_timestamp(row.timestamp, req.step, req.offset);
            let key = if req.fields.is_empty() {
                "*".to_string()
            } else {
                build_group_key(&row, &req.fields)
            };

            let entry = series.entry(key.clone()).or_insert_with(|| HitSeries {
                key,
                points: Vec::new(),
                total: 0,
            });
            entry.total += 1;
            entry.points.push(HitPoint {
                timestamp: ts,
                hits: 1,
            });
        }

        for entry in series.values_mut() {
            entry.points.sort_by_key(|p| p.timestamp);
            entry.points = merge_hits(entry.points.drain(..));
        }

        let mut values: Vec<HitSeries> = series.into_values().collect();
        values.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.key.cmp(&b.key)));
        if let Some(limit) = req.limit {
            values.truncate(limit);
        }

        Ok(values)
    }

    /// Returns facets (hits per value) for a single field.
    pub fn facets(&self, req: FacetRequest) -> crate::errors::Result<Vec<ValueWithHits>> {
        let rows = self.storage.run_query(&req.query)?;
        let mut counts: HashMap<String, u64> = HashMap::new();
        for row in rows {
            for f in row.fields {
                if f.name == req.field {
                    *counts.entry(f.value).or_default() += 1;
                }
            }
        }
        Ok(to_sorted_hits(counts, req.limit))
    }

    /// Runs a query and returns matching rows (after applying Query.limit).
    pub fn query(&self, query: &Query) -> crate::errors::Result<Vec<LogRow>> {
        self.storage.run_query(query)
    }
}

fn bucket_timestamp(ts: i64, step: i64, offset: i64) -> i64 {
    if step <= 0 {
        return ts;
    }
    ts - ((ts - offset) % step)
}

fn build_group_key(row: &LogRow, fields: &[String]) -> String {
    let mut parts = Vec::with_capacity(fields.len());
    for field in fields {
        let value = row
            .fields
            .iter()
            .find(|f| &f.name == field)
            .map(|f| f.value.as_str())
            .unwrap_or("*");
        parts.push(format!("{field}={value}"));
    }
    parts.join(",")
}

fn merge_hits(points: impl IntoIterator<Item = HitPoint>) -> Vec<HitPoint> {
    let mut merged: Vec<HitPoint> = Vec::new();
    for point in points {
        if let Some(last) = merged.last_mut() {
            if last.timestamp == point.timestamp {
                last.hits += point.hits;
                continue;
            }
        }
        merged.push(point);
    }
    merged
}

fn to_sorted_hits(mut counts: HashMap<String, u64>, limit: Option<usize>) -> Vec<ValueWithHits> {
    let mut values: Vec<ValueWithHits> = counts
        .drain()
        .map(|(value, hits)| ValueWithHits { value, hits })
        .collect();
    values.sort_by(|a, b| b.hits.cmp(&a.hits).then_with(|| a.value.cmp(&b.value)));
    if let Some(limit) = limit {
        if values.len() > limit {
            values.truncate(limit);
        }
    }
    values
}

fn is_stream_field(name: &str) -> bool {
    name.starts_with("_stream")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::StorageConfig,
        model::{FieldFilter, LogField},
        storage::VlStorage,
    };

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64
    }

    fn setup_storage() -> (VlStorage, i64) {
        let storage = VlStorage::new_local(StorageConfig::default()).unwrap();
        let base = now();
        let rows = vec![
            LogRow::new(
                base,
                vec![
                    LogField::new("level", "info"),
                    LogField::new("message", "first"),
                    LogField::new("_stream_app", "api"),
                ],
            ),
            LogRow::new(
                base + 1_000,
                vec![
                    LogField::new("level", "warn"),
                    LogField::new("message", "second"),
                    LogField::new("_stream_app", "api"),
                ],
            ),
            LogRow::new(
                base + 2_000,
                vec![
                    LogField::new("level", "info"),
                    LogField::new("message", "third"),
                    LogField::new("_stream_app", "worker"),
                ],
            ),
        ];
        storage.add_rows(rows).unwrap();
        (storage, base)
    }

    fn engine() -> (LogSqlEngine<'static>, i64) {
        let (storage, base) = setup_storage();
        let storage = Box::leak(Box::new(storage));
        (LogSqlEngine::new(storage), base)
    }

    #[test]
    fn field_names_and_values() {
        let (engine, _) = engine();
        let query = Query::default();

        let names = engine.field_names(&query).unwrap();
        let names: Vec<_> = names.into_iter().map(|v| v.value).collect();
        assert!(names.contains(&"level".to_string()));
        assert!(names.contains(&"message".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("_stream")));

        let values = engine.field_values(&query, "level", None).unwrap();
        assert_eq!(values[0].value, "info");
        assert_eq!(values[0].hits, 2);
    }

    #[test]
    fn stream_field_names_and_values() {
        let (engine, _) = engine();
        let query = Query::default();

        let names = engine.stream_field_names(&query).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].value, "_stream_app");

        let values = engine
            .stream_field_values(&query, "_stream_app", None)
            .unwrap();
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn hits_grouping_and_bucket() {
        let (engine, _) = engine();
        let req = HitsRequest {
            query: Query::default(),
            step: 1_000,
            offset: 0,
            fields: vec!["level".into()],
            limit: None,
        };
        let series = engine.hits(req).unwrap();
        assert_eq!(series.len(), 2);
        let info = series.iter().find(|s| s.key.contains("info")).unwrap();
        assert_eq!(info.points.len(), 2);
        assert_eq!(info.total, 2);
    }

    #[test]
    fn facets_and_time_range() {
        let (engine, base) = engine();
        let query = Query::default();
        let facets = engine
            .facets(FacetRequest {
                query: query.clone(),
                field: "level".into(),
                limit: None,
            })
            .unwrap();
        assert_eq!(facets.len(), 2);

        let range = engine.query_time_range(&query).unwrap().unwrap();
        assert_eq!(range, (base, base + 2_000));
    }

    #[test]
    fn query_with_filters() {
        let (engine, base) = engine();
        let query = Query {
            start: None,
            end: None,
            limit: None,
            filters: vec![FieldFilter::Equals {
                name: "level".into(),
                value: "warn".into(),
            }],
        };
        let rows = engine.query(&query).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].timestamp, base + 1_000);
    }
}
