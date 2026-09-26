# Changelog

All notable changes to `torajs-microtask` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial release, replacing the previous C microtask queue.
- `__torajs_microtask_enqueue(fn_, arg)` — append a task
  record to the queue tail.
- `__torajs_microtask_run_until_idle()` — drain to empty in FIFO
  order; tasks enqueued during the drain run in the same drain pass.
- `__torajs_microtask_pending_count()` — inspect queue length.
- Grow-by-doubling backing `Vec<TaskRecord>` + head-cursor pop +
  compaction at head > cap/2 to prevent unbounded queue growth
  on long-lived programs that enqueue + drain repeatedly.
