# torajs-playground-api

**Workspace-internal · non-metal-tier** — cloud server for torajs.com/playground.
Not a publishable crate.

`torajs-playground-api` is a thin HTTP API that accepts user TS source,
compiles via `tr build`, runs the resulting wasm under wasmtime
sandboxing, and returns captured stdout / stderr + compile + run
timing. Powers the playground page at https://torajs.com/playground.

This is **non-metal-tier** per the torajs design rules (see
`docs/deps-tree-v0.1.md` Glossary):

- Allowed Cargo deps from the application-tier ecosystem (axum,
  tokio, serde, sha2, tracing — see `Cargo.toml`).
- Vision rules (0 deps, 4.41× perf) explicitly relaxed.
- Long-term audit: may be moved out of this repo into a separate
  product repo if the workspace gets too heavy with non-metal
  concerns.

## Endpoints

```
POST /api/run
  body: { "source": "..." }
  response: { "status": "ok", "stdout": "...", "stderr": "...",
              "compile_ms": 12, "run_ms": 3, "cached": false }

GET /api/health
  response: { "status": "ok", "version": "..." }
```

## Sandboxing

- wasmtime fuel limit (CPU budget cap)
- Wall-clock timeout per request
- Tempdir per request (file I/O sandbox)
- `tower_governor` rate-limit
- 64 KiB max source size

## Caching

User-source → wasm cache keyed by SHA-256(source); wasm artifacts
stored under a configurable cache dir. Subsequent identical sources
skip recompile.

The SHA-256 → hex digest conversion uses **`torajs-codec-hex`** (F.1
ship, 2026-05-25) — replacing the `hex 0.4` community crate per
torajs vision priority #4 (0 deps).

## License

Workspace-internal. Non-metal-tier — Apache-2.0 / MIT headers but
not published to crates.io.
