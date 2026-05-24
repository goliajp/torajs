# torajs-panic

[![Crates.io](https://img.shields.io/crates/v/torajs-panic?style=flat-square&logo=rust)](https://crates.io/crates/torajs-panic)
[![docs.rs](https://img.shields.io/docsrs/torajs-panic?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-panic)
[![License](https://img.shields.io/crates/l/torajs-panic?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-panic?style=flat-square)](https://crates.io/crates/torajs-panic)

Fatal-error helper for AOT-compiled user binaries: `__torajs_panic(msg)`
writes the message to stderr, prints a symbolicated backtrace (macOS
`atos` / Linux raw PC chain), then `exit(1)`. Layer-1 substrate; 0
Cargo deps.

Extracted from the [torajs] AOT TypeScript runtime as **P7.i-panic**
(commit `7af88d6`, 2026-05-24) — the central fatal-error path used
by runtime helpers when an unrecoverable invariant violation occurs
(e.g. type_tag dispatch sees an impossible discriminant, refcount
goes negative on a non-static heap object). Replaces the C `__torajs_panic`
that previously lived in `runtime_str.c`.

## Difference from `torajs-abort`

| Crate | Purpose | Output | Use |
| --- | --- | --- | --- |
| `torajs-abort` | Bail on developer-error invariants in Rust staticlibs | Message + `\n` to stderr + `abort()` (signal 6) | Replaces `expect()` / `panic!()` / `assert!()` in staticlib bodies — never expected to fire at runtime |
| `torajs-panic` | Bail on runtime invariants in user binaries | Message + symbolicated backtrace + `exit(101)` | Called from IR-emitted code when an "impossible" state is observed at runtime — backtrace is for debugging |

If you need a backtrace, use `torajs-panic`. If you need to strip the
Rust panic infrastructure, use `torajs-abort`.

## Quick start

```rust
use std::ffi::CString;
use torajs_panic::__torajs_panic;

# fn _example() {
let msg = CString::new("impossible: type_tag is 42").unwrap();
unsafe { __torajs_panic(msg.as_ptr() as *const u8) };
// noreturn — process exits with code 1
# }
```

## Symbolication strategy

- **macOS**: backtraces via libc `backtrace(...)` get raw addresses;
  we shell out to `atos -o <exe>` to symbolicate (matches what the
  C version did). Uses `libc::system()` + `_NSGetExecutablePath` to
  locate the executable. The `atos` round-trip adds ~50 ms one-time
  on panic; irrelevant since the process is about to exit.
- **Linux**: raw PC chain via `backtrace(...)` + `backtrace_symbols(...)`
  (the addr2line decode path lives in `gimli`-style crates which we
  intentionally don't depend on; the printed text is `binary(addr)`
  shape and external `addr2line` users can post-process).
- **Other**: no backtrace; just the message.

## What it does NOT do

- **No panic recovery / hooks.** Process exits unconditionally.
  Callers that need graceful shutdown should not use this.
- **No Rust panic handler integration.** This is `extern "C"`, not
  Rust `panic!()`. `catch_unwind` won't catch it.
- **No symbol demangling for non-mangled C symbols.** The raw symbol
  table is what `atos` (macOS) / `backtrace_symbols` (Linux)
  produces; Rust-mangled names go through `_RNvCxx` chains untranslated
  on Linux. macOS `atos` handles Rust demangling.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
