---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Coding Style

> This file extends [common/coding-style.md](../common/coding-style.md) with Rust specific content.

## Formatting

- **rustfmt** with default settings — no custom `rustfmt.toml`
- **clippy** with default lints — no custom `clippy.toml`
- Run `cargo fmt` and `cargo clippy` before every commit

## Dependencies

- Share versions through `[workspace.dependencies]` in multi-crate workspaces — feature crates reference with `<dep>.workspace = true`
- Prefer the latest stable release of each crate; use `cargo outdated` to check for drift, `cargo audit` + `cargo deny check` before adding a new dependency

## Error Handling

Use **thiserror** for all custom error types. Define explicit error enums at module boundaries:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}
```

- Map errors at boundaries with `From` impls or `.map_err()`
- Provide context-rich error messages
- Never silently swallow errors
- Log errors with `tracing::error!` before converting to user-facing messages

## Async Runtime

- **tokio** with `features = ["full"]` as the standard async runtime
- All I/O operations must be async
- Use `tokio::spawn` for background tasks
- Use `tokio::select!` for concurrent operations

## Naming

- PascalCase for types, enums, traits
- snake_case for functions, methods, variables, modules
- UPPER_SNAKE_CASE for constants
- Descriptive names: `find_path`, `place_order`, `broadcast_to_channel`

## Module Organization

- Organize by **feature/domain**, not by type
- Small focused files: 50-150 lines typical, 400 max
- Public facade pattern: `core/lib.rs` re-exports from internal crates
- Use `mod.rs` with `pub mod` for flat module exports

## Immutability

- Default-immutable: take `&self` and `&T` references
- Local `mut` only when algorithms require it
- Return new values instead of mutating inputs
- Use `Arc<T>` for shared ownership across async tasks
- Use `DashMap` or similar for concurrent mutable collections

## Derive Usage

Apply standard derives liberally:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config { ... }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status { ... }
```

- `#[serde(skip)]` for sensitive fields
- `#[serde(rename_all = "snake_case")]` for enum variants
