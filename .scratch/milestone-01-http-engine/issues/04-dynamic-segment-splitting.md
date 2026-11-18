# 04: Dynamic Segment splitting (work stealing)

**What to build:** Add shared `AtomicU64` end boundaries to active `Segment`s so that when a `Worker` finishes early, the scheduler bisects the largest remaining `Segment` above the 2 MiB minimum floor, assigns the upper half to the idle `Worker`, and causes the incumbent `Worker` to stop reading and drop its HTTP stream once its `cursor` reaches `mid`.

**Blocked by:** 03: Multi-Segment concurrent Download with pre-allocated .part file

**Status:** ready-for-agent

- [ ] Finished `Worker`s steal the upper half of the largest remaining `Segment` when the remaining un-downloaded range exceeds the 2 MiB floor.
- [ ] Incumbent `Worker`s check the shared `AtomicU64` end boundary as `Chunk`s arrive and truncate/drop their HTTP stream at `mid` without issuing extra requests.
- [ ] Integration test with an artificially throttled `Worker` verifies that faster `Worker`s split its remaining range and complete the `Download` with byte-for-byte BLAKE3 integrity.
- [ ] `cargo test` and `cargo clippy -- -D warnings` pass cleanly.
