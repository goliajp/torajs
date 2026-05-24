# torajs-fetch

[![Crates.io](https://img.shields.io/crates/v/torajs-fetch?style=flat-square&logo=rust)](https://crates.io/crates/torajs-fetch)
[![docs.rs](https://img.shields.io/docsrs/torajs-fetch?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-fetch)
[![License](https://img.shields.io/crates/l/torajs-fetch?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-fetch?style=flat-square)](https://crates.io/crates/torajs-fetch)

Synchronous HTTP fetch for the [torajs] AOT TypeScript runtime: minimal
libcurl-easy wrapper exposing `__torajs_fetch_sync(url) -> Response`
and `__torajs_response_drop`. Native target only; `wasm32-wasi`
target gets an empty-body Response shape (browser-fetch routing
will live in a sibling crate post-v0.6).

Extracted from `runtime_fetch.c` (~340 LOC) as **P6.3** (commit
`f04a2dc`, 2026-05-24). Paired with `torajs-promise` at codegen —
the TS `fetch(url)` lowers to:

```
promise_alloc_fulfilled_heap(
    __torajs_fetch_sync(url)
)
```

## Why synchronous (for now)

torajs's runtime is single-threaded today and exposes a Promise-
shaped JS API. The fetch implementation runs synchronously under
the hood (`curl_easy_perform` blocks the caller) but is wrapped at
codegen into a Promise so user code follows the expected `.then` /
`await` shape. When threading lands post-v1.0, the fetch backend
becomes a proper non-blocking implementation; user code doesn't
change.

## Response layout

```text
Response = [header:8][status:8][body:Str]
```

- `header:8` — universal heap header (`refcount + type_tag=Response +
  flags`).
- `status:8` — HTTP status code as i64 (200, 404, 500, ...).
- `body` — Str pointer (refcount-shared); empty Str on error.

`__torajs_response_drop` decrements the body Str's refcount and
frees the Response block.

## ABI

```rust
// On native, performs the GET via libcurl-easy and returns the
// Response. On wasm32-wasi, returns an empty-body Response with
// status 0 (T-21.b will route through the browser fetch API).
pub unsafe extern "C" fn __torajs_fetch_sync(url: *const u8) -> *mut c_void;

pub unsafe extern "C" fn __torajs_response_drop(p: *mut c_void);
```

## What's NOT in scope (v0.1.0)

- **HTTP methods other than GET**: POST / PUT / DELETE / PATCH are
  not yet supported. Add when a user-code path needs them.
- **Headers**: no request- or response-header API. Body-only.
- **Streaming response**: full body materialized before the
  Response is returned.
- **TLS configuration**: uses libcurl defaults (system CA bundle).
- **Cookies / authentication / timeouts**: not yet configurable.
- **Async / cancellation**: synchronous-only as documented above.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
