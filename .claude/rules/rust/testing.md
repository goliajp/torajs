---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Testing

> Extends `common/testing.md` with Rust-specific content. Read the common file first for coverage targets, TDD workflow, and "what to test / never mock" guidance.

## Test Placement

Inline `#[cfg(test)]` modules in the same file as the code under test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get() {
        let mut reg = Registry::new();
        reg.register("foo", 42);
        assert_eq!(reg.get("foo"), Some(&42));
    }
}
```

## Async Tests

Use `#[tokio::test]` for async test functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_user_returns_ok() {
        let pool = setup_test_db().await;
        let user = User::create(&pool, "alice").await.unwrap();
        assert_eq!(user.username, "alice");
    }
}
```

## Readability

- One concept per test, clear descriptive names
- Use helper functions within test modules to reduce boilerplate:
  ```rust
  fn make_order(side: OrderSide, price: f64, qty: u64) -> Order {
      Order { id: Uuid::new_v4(), side, price: Some(price), qty, .. }
  }
  ```
- Arrange-Act-Assert pattern with blank lines separating sections
- Prefer `assert_eq!` over `assert!` for better failure messages

## Coverage

```bash
cargo test
cargo tarpaulin --out html   # coverage report
```
