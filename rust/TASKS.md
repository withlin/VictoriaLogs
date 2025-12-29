# Rust vmstorage Migration Tasks

## Phase 1: Storage Foundations (disk layout + writes)
- [x] Define disk format: partitions/parts, column blocks, metadata, index files (map Go small/big parts + inmemory flush triggers)
- [x] Implement write pipeline: in-memory batches, WAL/flush, rate limiting, read-only checks
- [x] Checkpoint A: logs persist to disk, survive restart, retention (time/future/backfill) enforced

## Phase 2: Indexing & Query Execution
- [ ] Build column/inverted indexes (field -> offsets), time filtering; field/stream value lookups
- [ ] Implement query executor (parallel readers, offset/limit, sorting, lastN optimization)
- [ ] Checkpoint B: core query APIs return correct results; field/stream names & values endpoints work

## Phase 3: Merges & Compression
- [ ] Implement small->big part merge strategy, zstd compression, fragmentation handling; background merge scheduling
- [ ] Controls for pausing/canceling merges; force merge/flush APIs
- [ ] Checkpoint C: merges run under sustained writes, space reclaimed, force merge/flush effective

## Phase 4: Management & Deletes
- [ ] /internal/* routes: log_new_streams, force_merge/flush, partition attach/detach/snapshot/list
- [ ] Delete tasks (start/stop/active) and tenant enumeration
- [ ] Checkpoint D: HTTP internal APIs and delete tasks pass self-tests

## Phase 5: Network/Cluster Mode
- [ ] Implement netinsert/netselect protocol (compression, auth, TLS, partial responses, concurrency)
- [ ] Checkpoint E: cross-node read/write works; partial response semantics match Go

## Phase 6: Observability & Config
- [ ] Metrics exposure (parity with Go), logging/rate limiting/read/write status
- [ ] Complete CLI/flag coverage (retention, disk quotas, concurrency, compression, auth, etc.)
- [ ] Checkpoint F: metrics/config align with Go; basic monitoring operational
