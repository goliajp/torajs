# Changelog

All notable changes to `torajs-fetch` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial release, replacing the previous C implementation.
- `__torajs_fetch_sync(url) -> *mut c_void` — synchronous HTTP
  GET via libcurl-easy. Returns a Response heap block on success
  (status + body Str); empty-body Response with status 0 on
  network errors.
- `__torajs_response_drop(p)` — decrement body Str refcount + free
  the Response block.
- Native target uses libcurl (`#[link(name = "curl")]`); wasm32-
  wasi target degrades to empty-body Response shape.
