---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Security

> This file extends [common/security.md](../common/security.md) with Rust specific content.

## Secret Management

Load from environment, fail fast on missing required secrets:

```rust
let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET required");
let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
```

- Document required variables in `.env.example`; never commit `.env`
- Mark sensitive struct fields with `#[serde(skip)]` so they never leak through serialization (password hashes, tokens, API keys)

## Dependency Auditing

```bash
cargo audit          # known vulnerability scan
cargo deny check     # license + advisory checks
```

## SQL Injection Prevention

Always use parameterized queries via sqlx:

```rust
sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
    .bind(user_id)
    .fetch_one(pool)
    .await?;
```

Never use `format!` to build SQL strings.

## Auth Patterns

- `argon2` for password hashing
- `jsonwebtoken` for JWT (dual token: short-lived access + long-lived refresh)
- Validate tokens in middleware, not in handlers
- Error messages must not leak internal details:
  ```rust
  // GOOD: generic message to client
  ApiError::Unauthorized
  // BAD: leaking internals
  ApiError::Message(format!("token decode failed: {e}"))
  ```

## Unsafe Code

- Avoid `unsafe` unless absolutely necessary for FFI or performance-critical paths
- Document why `unsafe` is sound with a `// SAFETY:` comment
- Prefer safe abstractions from crates (e.g., `ringbuf` over raw pointer manipulation)
