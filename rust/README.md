# VictoriaLogs Rust Workspace

This workspace is for the Rust rewrite of VictoriaLogs components, split into multiple crates:

- `logstorage`: shared library with storage primitives and LogSQL helpers.
- `vmstorage`: binary scaffold to host storage in local mode.
- `vminsert`: binary scaffold for ingestion (stubbed; forwards to storage later).
- `vmselect`: binary scaffold for querying (stubbed; queries storage later).

## Getting started

- Build and run tests: `cargo check` or `cargo test` from `rust/`.
- The crate is a library; it exposes helpers to serve `/internal/*` routes and manage storage state in-memory.

## Status and next steps

- Local mode is backed by an in-memory engine that implements the admin endpoints and basic querying.
- LogSQL-style helpers (hits, facets, field/stream metadata) run against the same in-memory engine.
- Cluster/network mode is stubbed; ingestion/query paths for remote nodes are placeholders.
- Planned follow-ups:
  1. Persist data to disk and add retention/compaction primitives.
  2. Wire HTTP handling to a real server layer (e.g. axum/hyper) and mirror auth semantics.
  3. Implement network ingestion/query paths with compression/auth options to match Go `netinsert`/`netselect`.
  4. Expose metrics and richer error reporting to keep parity with the existing binary.
