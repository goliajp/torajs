# torajs-process performance budgets

Latency dominated by libc syscall cost (getcwd / getenv / write /
exit). Rust wrapper overhead is < 100 ns per call.

| Path | Budget (Rust-side) | Notes |
| --- | ---: | --- |
| `process_exit` | < 10 ns to wrapper exit; libc exit() then runs | One-shot. |
| `process_cwd` | < 1 µs | getcwd into a Str — typical paths < 1 KB. |
| `process_env_get` | < 200 ns | getenv pointer lookup + null check + Str alloc. |
| `process_argv` | < 5 µs first-call | Lazy-init: argv parse + Str alloc per arg + Array<Str> build. Cached after first call. |
| `process_platform` | < 50 ns | Returns a static `.rodata` Str pointer. |
| `process_stdout_write` / `_stderr_write` | < 1 µs | Locked write + libc write(2). |

## What's NOT budgeted

- **Syscall latency itself**: OS-dependent (write may block on
  buffered I/O or tty).
- **`process.argv` mutation**: returns a fresh Array<Str> on
  first access; mutations don't propagate back to libc argv.
