# torajs-throw

[![Crates.io](https://img.shields.io/crates/v/torajs-throw?style=flat-square&logo=rust)](https://crates.io/crates/torajs-throw)
[![docs.rs](https://img.shields.io/docsrs/torajs-throw?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-throw)
[![License](https://img.shields.io/crates/l/torajs-throw?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-throw?style=flat-square)](https://crates.io/crates/torajs-throw)

Catchable-throw infrastructure for the [torajs] AOT TypeScript runtime:
TLS-shaped throw slot + 3-slot native-error factory registry (Error /
TypeError / RangeError). Layer-1 substrate; 0 Cargo deps.

Powers JS-style `try / catch` propagation through `extern "C"` runtime
helpers: a Rust-side helper that wants to raise (e.g. `bigint /
0` → RangeError, `assign-to-frozen-prop` → TypeError) calls
`__torajs_throw_range_error("msg")`, which builds a real catchable
Error-subclass instance via the registered factory and stores it in
the TLS throw slot. Caller-site IR emits a poll (`__torajs_throw_check`)
after the call and unwinds via the `emit_throw_check` SSA pass when
non-zero — no `setjmp` / `longjmp`, no Rust `panic!` integration.

## Why TLS slot rather than longjmp / unwind

- **AOT toolchain neutrality.** No platform-specific unwind table
  emission; the IR-emitted poll lives at every `bl helper + ret`
  boundary the codegen knows might throw.
- **Tight cold path.** The TLS check is one load + cbnz. Happy path
  pays a single conditional branch.
- **Single-threaded.** torajs is single-threaded today; the "TLS
  slot" is currently a plain `static AtomicI64` pair. Atomics only
  for Rust's safety story.

## API surface

3 categories of ABI:

### Throw slot ops

```rust
pub unsafe extern "C" fn __torajs_throw_set(tag: i64, value: i64);
pub unsafe extern "C" fn __torajs_throw_check() -> i64;     // 1 if pending
pub unsafe extern "C" fn __torajs_throw_take() -> i64;      // pops value
pub unsafe extern "C" fn __torajs_throw_take_tag() -> i64;  // pops tag
```

### Native-error registry

```rust
pub const SLOT_ERROR: usize       = 0;
pub const SLOT_TYPE_ERROR: usize  = 1;
pub const SLOT_RANGE_ERROR: usize = 2;

pub type NativeErrorFactory =
    unsafe extern "C" fn(message_str: *mut c_void) -> *mut c_void;

pub unsafe extern "C" fn __torajs_register_native_error(
    slot: i64, fnptr: *mut c_void,
);
```

The factory is the codegen-emitted `__new_<C>(message)` of each
builtin Error subclass; `synthesize_module_init` registers them
during program startup.

### Convenience throwers

```rust
pub unsafe extern "C" fn __torajs_throw_range_error(msg: *const c_char);
pub unsafe extern "C" fn __torajs_throw_type_error(msg: *const c_char);
```

Cross-translation-unit helpers; bigint / regex / dynobj / numeric /
... runtime crates call these to raise spec-mandated catchable errors.
Falls back to a bare-string throw if no factory is registered (e.g.
during very early startup before the Error class has been
synthesized).

## What this crate does NOT do

- **No `panic!()` integration.** This is `extern "C"` throw machinery,
  not Rust panic. `catch_unwind` won't catch a `throw_range_error`
  call; that's by design.
- **No backtrace.** The thrown Error instance has a `.stack` field
  filled by the codegen-emitted factory (which captures the user-
  code frame), but this crate's helpers don't unwind any frames
  themselves.
- **No unwind table emission.** AOT toolchain doesn't pay for any
  DWARF unwind metadata related to this; the poll-based unwinding is
  done entirely in user-IR.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
