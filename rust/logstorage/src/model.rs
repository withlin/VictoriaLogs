use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogField {
    pub name: String,
    pub value: String,
}

impl LogField {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogRow {
    pub timestamp: i64,
    pub fields: Vec<LogField>,
}

impl LogRow {
    pub fn new(timestamp: i64, fields: Vec<LogField>) -> Self {
        Self { timestamp, fields }
    }

    pub fn matches(&self, query: &Query) -> bool {
        if let Some(start) = query.start {
            if self.timestamp < start {
                return false;
            }
        }

        if let Some(end) = query.end {
            if self.timestamp > end {
                return false;
            }
        }

        query.filters.iter().all(|filter| filter.matches(self))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FieldFilter {
    Equals { name: String, value: String },
}

impl FieldFilter {
    pub fn matches(&self, row: &LogRow) -> bool {
        match self {
            FieldFilter::Equals { name, value } => row
                .fields
                .iter()
                .any(|f| f.name == *name && f.value == *value),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Query {
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub limit: Option<usize>,
    pub filters: Vec<FieldFilter>,
}

impl Query {
    pub fn with_limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartitionSnapshot {
    pub partition: String,
    pub path: String,
    pub created_at_ms: u128,
}

impl PartitionSnapshot {
    pub fn new(partition: impl Into<String>, path: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        Self {
            partition: partition.into(),
            path: path.into(),
            created_at_ms: now,
        }
    }
}
