# torajs-abort

[![Crates.io](https://img.shields.io/crates/v/torajs-abort?style=flat-square&logo=rust)](https://crates.io/crates/torajs-abort)
[![docs.rs](https://img.shields.io/docsrs/torajs-abort?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-abort)
[![License](https://img.shields.io/crates/l/torajs-abort?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-abort?style=flat-square)](https://crates.io/crates/torajs-abort)

Panic-free abort helper for staticlib-shipped Rust crates. `no_std`. 0
Cargo deps — libc `write(2)` + `abort()` declared inline via
`unsafe extern "C"`.

Extracted from the [torajs] AOT TypeScript runtime where it shipped as
polish A3 (commit `ab7a286`, 2026-05-24) to strip the Rust `std::panic`
infrastructure (`std::backtrace` / `gimli` / `addr2line` /
`rustc_demangle` / `std::io::Error` / `std::thread::Thread`) from
every user binary. Combined with build-std + panic=abort, the
measured user-binary delta on the torajs bench corpus is **~150 KB
per binary**.

## When to use

Inside a `staticlib`-typed crate whose object gets linked into a final
user binary where Rust's default panic machinery is dead weight: each
`expect()` / `panic!()` / `assert!()` call drags in the panic-handler
chain transitively, even if the panic path never runs at runtime.

`abort_with(msg)` replaces those calls with a 2-syscall sequence
(`write(2, msg, len)` + `write(2, "\n", 1)` + `abort()`) that produces
the same observable failure UX (message on stderr + non-zero exit)
without the panic-handler tree.

## Quick start

```rust
use torajs_abort::abort_with;

// Replace:
//   let v = some_option.expect("oom");
// With:
let some_option: Option<i32> = None;
# fn _example() {
let v = some_option.unwrap_or_else(|| abort_with(b"oom"));
# let _ = v;
# }

// Replace:
//   assert!(idx < len, "OOB");
// With:
# fn _example2(idx: usize, len: usize) {
if idx >= len { abort_with(b"OOB"); }
# }
```

## Why `&[u8]` rather than `&str`

The message API takes a byte slice intentionally:

- No UTF-8 validation cost on the abort path.
- Lets callers ship pre-computed byte-string literals
  (`b"InvalidArgument"`) — no allocator dependency at the fail site.
- The `write(2)` interface itself is byte-oriented; making the API
  match removes a conversion step.

## What it does NOT do

- **No panic handler integration.** This is `extern "C"` not Rust
  panic. Catch-unwind / panic-hook callers will not see this fail
  through their hooks; you're skipping the panic infra by design.
- **No backtrace symbolication.** If you need a backtrace, use plain
  `panic!()` — the whole point of this crate is to avoid pulling
  backtrace infra into the binary.
- **No structured error reporting.** Message is one byte slice; no
  `Display` / `Debug` / formatting.

## ABI

The crate exposes one `extern "C"` symbol:

```c
__torajs_abort_with(const uint8_t *msg, uintptr_t len) -> noreturn
```

C callers can link `libtorajs_abort.a` and call this directly. Rust
callers use the ergonomic `abort_with(&[u8])` wrapper which forwards
to the no-mangle extern.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
