# torajs-fs performance budgets

Latency is dominated by OS-level syscall cost (open / read / write /
unlink / mkdir / stat / readdir) + page-cache state. The Rust wrapper
adds < 100 ns per call.

## Per-op budgets (Rust-side overhead only; syscall excluded)

| Path | Budget | Observed (dev) | Notes |
| --- | ---: | ---: | --- |
| Path-Str → C string conversion + null-term scratch | ≤ 100 ns | ~50 ns | One PATH_MAX-bounded stack buffer; no alloc. |
| Body Str alloc (read paths) | ≤ syscall RTT | per file size | Single `Str::alloc` + memcpy from kernel buffer. |
| Body refcount-dec (write paths) | ≤ 100 ns | ~30 ns | One refcount-dec + maybe free. |

## Syscall latency (informational only; out of scope)

| Op | Approx (dev) | Notes |
| --- | ---: | --- |
| `read_file_sync` (1 KB, page-cache hit) | ~5 µs | open + read + close |
| `read_file_sync` (1 MB, page-cache hit) | ~500 µs | dominated by memcpy |
| `read_file_sync` (1 KB, cold) | ~ms | disk read |
| `write_file_sync` (1 KB, fsync=off) | ~10 µs | open + write + close |
| `stat_size_sync` | ~1 µs | fast vmstat lookup |
| `readdir_sync` (small dir) | ~10 µs | opendir + readdir loop + closedir |

## What's NOT budgeted

- **Syscall latency itself**: OS / disk / page-cache dependent.
- **Concurrent fs access**: no per-handle locking — caller's
  responsibility.
- **Large-file streaming**: full body materialized into a single
  Str — bounded by available memory.
