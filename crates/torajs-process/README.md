# torajs-process

[![Crates.io](https://img.shields.io/crates/v/torajs-process?style=flat-square&logo=rust)](https://crates.io/crates/torajs-process)
[![docs.rs](https://img.shields.io/docsrs/torajs-process?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-process)
[![License](https://img.shields.io/crates/l/torajs-process?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-process?style=flat-square)](https://crates.io/crates/torajs-process)

Process-surface helpers for the [torajs] AOT TypeScript runtime — the
Node.js-compatible `process.*` API. 0 Cargo deps.

Extracted from `runtime_str.c`'s process family (~240 LOC) as
**P7.h-proc** (commit `5527de7`, 2026-05-24).

## Surface (Node.js-compatible subset)

| TS / Node API | extern `__torajs_*` symbol | Notes |
| --- | --- | --- |
| `process.exit(code)` | `__torajs_process_exit` | libc exit |
| `process.cwd()` | `__torajs_process_cwd` | getcwd → Str |
| `process.env[key]` | `__torajs_process_env_get` | getenv → Str or null |
| `process.argv` | `__torajs_process_argv` | Lazy-initialized Array<Str> from main args |
| `process.platform` | `__torajs_process_platform` | "darwin" / "linux" / ... Str |
| `process.stdout.write(s)` | `__torajs_process_stdout_write` | Locked stdout write |
| `process.stderr.write(s)` | `__torajs_process_stderr_write` | Locked stderr write |

## What it does NOT do (v0.1.0)

- **stdin reading**: out of scope for the CLI-first MVP.
- **process.kill / process.pid**: not needed in the current TS surface.
- **process.on('exit', ...) hooks**: not yet wired into the
  microtask drain.
- **process.versions**: would require version-info baked into the
  build; deferred.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
