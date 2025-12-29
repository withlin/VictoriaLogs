//! Rust rewrite scaffold for `vlstorage`.
//!
//! This crate mirrors the public surface of the Go implementation in a small,
//! testable package so we can iterate on a Rust port without disrupting the
//! existing codebase.

pub mod config;
pub mod errors;
pub mod logsql;
pub mod model;
pub mod request;
pub mod storage;

pub use config::StorageConfig;
pub use errors::{Result, StorageError};
pub use logsql::{FacetRequest, HitPoint, HitSeries, HitsRequest, LogSqlEngine, ValueWithHits};
pub use model::{FieldFilter, LogField, LogRow, PartitionSnapshot, Query};
pub use request::{Request, Response};
pub use storage::{NetStorageNode, StorageMode, VlStorage};

/// Handles the `/internal/*` endpoints that existed in the Go implementation.
///
/// Returns `Some(Response)` if the path is known, otherwise `None` so callers
/// can forward the request to other handlers.
pub fn handle_internal_request(storage: &VlStorage, req: &Request) -> Option<Response> {
    use request::ResponseKind;

    match req.path.as_str() {
        "/internal/log_new_streams" => {
            let seconds = req.query_u64("seconds").unwrap_or(10);
            Some(match storage.log_new_streams(seconds) {
                Ok(_) => Response::empty(ResponseKind::Ok),
                Err(err) => Response::error(err),
            })
        }
        "/internal/force_merge" => {
            let prefix = req.query.get("partition_prefix").cloned();
            Some(match storage.force_merge(prefix) {
                Ok(_) => Response::empty(ResponseKind::Accepted),
                Err(err) => Response::error(err),
            })
        }
        "/internal/force_flush" => Some(match storage.force_flush() {
            Ok(_) => Response::empty(ResponseKind::Accepted),
            Err(err) => Response::error(err),
        }),
        "/internal/partition/attach" => {
            let Some(name) = req.query.get("name").cloned() else {
                return Some(Response::error(StorageError::MissingParameter("name")));
            };
            Some(match storage.partition_attach(name) {
                Ok(_) => Response::empty(ResponseKind::Ok),
                Err(err) => Response::error(err),
            })
        }
        "/internal/partition/detach" => {
            let Some(name) = req.query.get("name").cloned() else {
                return Some(Response::error(StorageError::MissingParameter("name")));
            };
            Some(match storage.partition_detach(name) {
                Ok(_) => Response::empty(ResponseKind::Ok),
                Err(err) => Response::error(err),
            })
        }
        "/internal/partition/list" => Some(match storage.partition_list() {
            Ok(names) => Response::json(ResponseKind::Ok, &names),
            Err(err) => Response::error(err),
        }),
        "/internal/partition/snapshot/create" => {
            let Some(name) = req.query.get("name").cloned() else {
                return Some(Response::error(StorageError::MissingParameter("name")));
            };
            Some(match storage.partition_snapshot_create(name) {
                Ok(snapshot) => Response::json(ResponseKind::Ok, &snapshot),
                Err(err) => Response::error(err),
            })
        }
        "/internal/partition/snapshot/list" => Some(match storage.partition_snapshot_list() {
            Ok(snapshots) => Response::json(ResponseKind::Ok, &snapshots),
            Err(err) => Response::error(err),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_partition_list_request() {
        let storage = VlStorage::new_local(StorageConfig::default()).unwrap();
        storage.partition_attach("p1".into()).unwrap();
        storage.partition_attach("p2".into()).unwrap();

        let req = Request {
            path: "/internal/partition/list".into(),
            query: Default::default(),
        };

        let resp = handle_internal_request(&storage, &req).unwrap();
        assert_eq!(resp.status, 200);
        assert!(resp.body.unwrap().contains("p1"));
    }
}
