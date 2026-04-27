---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Patterns

## Workspace Organization

Multi-crate workspaces with centralized dependency management:

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2021"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
tracing = "0.1"
```

## Config Pattern

Environment-driven, Clone-able for sharing across async tasks:

```rust
#[derive(Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub port: u16,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL required"),
            port: std::env::var("PORT").unwrap_or_else(|_| "8080".into()).parse().unwrap(),
        }
    }
}
```

## API Response Envelope

Consistent JSON response wrapper:

```rust
#[derive(Serialize)]
pub struct OkResponse<T: Serialize> {
    ok: bool,
    #[serde(flatten)]
    data: T,
}
```

## Concurrency Patterns

- `Arc<T>` for shared ownership across async tasks
- `DashMap` for concurrent hashmaps (no explicit locking)
- `tokio::broadcast` for pub/sub channels
- `rayon` for CPU-bound data parallelism

## Database (sqlx)

- `sqlx::query_as` with parameter binding
- Constraint-aware error mapping at boundaries:
  ```rust
  .map_err(|e| match e {
      sqlx::Error::Database(ref db) if db.constraint() == Some("users_username_key") => {
          ApiError::Conflict("username already taken".into())
      }
      _ => ApiError::from(e),
  })
  ```

## Performance Patterns

- **Profile optimization**: `opt-level = 1` for dev, `opt-level = "s"` or `3` for release
- **Thin/Fat LTO**: use `lto = "thin"` for balanced builds, `lto = "fat"` for max perf
- **Integer arithmetic**: avoid floats in hot paths — convert to fixed-point (cents, basis points)
  ```rust
  fn to_cents(price: f64) -> i64 { (price * 100.0).round() as i64 }
  ```
- **Pre-allocated strings**: `String::with_capacity()` in hot paths
- **Ring buffers**: `ringbuf` for lock-free producer/consumer (real-time audio, streaming)
- **Collection choices**: `BTreeMap` for ordered iteration, `HashMap` for lookups, `VecDeque` for queues

## Performance Gates

For latency-sensitive code paths, pair `criterion` benchmarks with integration tests that `assert!` on fixed budgets. Criterion alone does not fail CI on regression; perf-gate tests do.

- `tests/perf_gate.rs` — integration tests asserting `elapsed < budget`, runs via `cargo test --workspace`, fails CI on regression
- `benches/` — criterion for detailed tracking (flamegraphs, trends)
- `BUDGETS.md` — per-crate table documenting each budget and its derivation

**Path taxonomy:**

- **Hot** — runs every frame / key press; budget in µs, < 16 ms total for 60 fps
- **Warm** — per user action (keystroke, click); budget ≤ 10 ms
- **Cold** — async / background I/O; no frame budget, log slow operations

**Budget rules:**

- Set budgets with **3× CI headroom** over observed P95 on a dev machine
- Never weaken a budget without re-measuring P95 and justifying in the commit message
- Never skip perf tests to "save CI time"

```rust
// tests/perf_gate.rs
#[test]
fn insert_char_100k_lines_under_30us() {
    let mut doc = Document::from_text(&"x\n".repeat(100_000));
    let start = Instant::now();
    doc.insert_char(Position::zero(), 'a');
    assert!(start.elapsed() < Duration::from_micros(30));
}
```

Reference: `goliajp/tora` — `rules/performance.md` + `crates/*/tests/perf_gate.rs` cover 4 crates with working budget tables.

## Conditional Compilation

Gate optional features cleanly:

```rust
#[cfg(feature = "gpu")]
pub mod gpu_heat;
```

## Logging

- `tracing` + `tracing-subscriber` for structured logging
- `tracing::error!` for errors, `tracing::info!` for lifecycle events
- Configure via `RUST_LOG` environment variable
