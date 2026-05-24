# torajs-meta

[![Crates.io](https://img.shields.io/crates/v/torajs-meta?style=flat-square&logo=rust)](https://crates.io/crates/torajs-meta)
[![docs.rs](https://img.shields.io/docsrs/torajs-meta?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-meta)
[![License](https://img.shields.io/crates/l/torajs-meta?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-meta?style=flat-square)](https://crates.io/crates/torajs-meta)

Runtime metadata + reflection substrate for the [torajs] AOT TypeScript
runtime. Three concerns rolled together:

1. **`fnprops`** — side-table mapping function instances → property
   bags (so `fn.foo = "bar"; fn.foo` works on user-declared fns).
2. **`classmeta`** — class-tag-keyed registries: class name lookup,
   prototype object retrieval, instanceof check.
3. **`reflect`** — `Object.getOwnPropertyDescriptor` / `getPrototypeOf` /
   `setPrototypeOf` reflection ops.

Extracted from `runtime_str.c`'s fnprops + class/proto + reflect
families (~510 LOC) as **P7.g** (commit `a4093e8`, 2026-05-24).

## Module layout

| Module | LOC | Purpose |
| --- | ---: | --- |
| `fnprops.rs` | ~150 | Side-table mapping (fn_ptr → property bag); used because closures don't have their own heap layout for arbitrary props |
| `classmeta.rs` | ~250 | class-tag → class-name lookup; class-tag → prototype object lookup; instanceof check via the prototype chain |
| `reflect.rs` | ~100 | Object.* reflection ops + small adapter glue |

## Cross-tier deps

| extern | Provider | Notes |
| --- | --- | --- |
| `__torajs_str_alloc_pooled` | torajs-str | for class name / property name Str blocks |
| `__torajs_rc_dec` | torajs-rc | refcount-dec on dropped owner pointers |

## What's NOT in scope

- **Decorator metadata** (TS `experimentalDecorators`): not yet
  surfaced — decorators lower to non-meta machinery today.
- **Object.defineProperty**: writeable / enumerable / configurable
  bits are not yet exposed; reflective access is read-only.
- **Module-level metadata**: registries are flat process-global;
  per-module separation lands when ES modules separate-realm
  semantics ship.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
