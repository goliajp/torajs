# torajs-str

[![Crates.io](https://img.shields.io/crates/v/torajs-str?style=flat-square&logo=rust)](https://crates.io/crates/torajs-str)
[![docs.rs](https://img.shields.io/docsrs/torajs-str?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-str)
[![License](https://img.shields.io/crates/l/torajs-str?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-str?style=flat-square)](https://crates.io/crates/torajs-str)

Core substrate of the [torajs] AOT TypeScript runtime: the `Str` +
`Substr` heap types and ALL spec-mandated string operations
(`String.prototype.*`, `JSON.parse` of string body, symbol family,
print). Pure Rust (`std`-only); 2 Cargo deps (`torajs-rc` for the
universal heap header + refcount, `torajs-abort` to keep the panic
infra out of the user binary).

## Layout

```text
Str    = [header:8][len:8][bytes:N]           prefix 16
Substr = [header:8][len:8][parent:8][off:8]   prefix 32
```

The 8-byte universal header (`refcount:u32 + type_tag:u16 + flags:u16`)
is supplied by `torajs-rc`; the rest is `torajs-str`'s. `STR_HDR_SIZE`
= 16; `SUBSTR_HDR_SIZE` = 32. Bytes are not NUL-terminated; the `len`
field is authoritative.

## Modules

| Module | Purpose |
| --- | --- |
| `layout.rs` | ABI offset / size constants + packed header init |
| `alloc.rs` | Pool-aware `alloc` + `free_pool_aware` + `__torajs_str_*` extern wrappers |
| `pool.rs` | Small-Str LIFO pool (32 slots × 16-byte payload) |
| `concat.rs` | `a + b` for Str operands; single alloc + 2 copies |
| `slice.rs` | `String.prototype.slice / substring / substr` with ES negative-index normalization |
| `lookup.rs` | `charCodeAt` / `startsWith` / `endsWith` / `indexOf` / `includes` |
| `eq.rs` | `===` / `!==` byte-equality, with FLAG_STATIC_LITERAL fast path |
| `print.rs` | stdout / stderr Str + Substr printing (locked + ordered with print_i64 etc.) |
| `literals.rs` | static .rodata Str-shaped globals + their FLAG_STATIC_LITERAL handling |
| `to_number.rs` | `Number(s)` ES §7.1.4 conversion |
| `json.rs` | `JSON.stringify` string body (quote escape, octal escape, JSON-specific escapes) |
| `json_parse/` | `JSON.parse` of string body (escape decode, unicode escape, surrogate pair handling) |
| `transform/` | `toUpperCase` / `toLowerCase` / `repeat` / `padStart` / `padEnd` / `trim` |
| `split/` | `split(sep)` + `split(/regex/)` → `Array<Substr>` |
| `substr.rs` | `Substr` heap type + `SubstrBlock` + pool ops |
| `substr_methods.rs` | Substr-typed `charCodeAt` / `slice` / etc. (view-aware dispatch) |
| `symbol.rs` | `Symbol` family (description / toString / valueOf) |

## ABI invariants (must not change)

| Symbol | Notes |
| --- | --- |
| `STR_HDR_SIZE` = 16 | Universal 8-byte header + 8-byte len |
| `STR_LEN_OFF` = 8 | u64 byte-length |
| `STR_DATA_OFF` = 16 | First payload byte |
| Packed init: `1u64 \| ((Tag::Str as u64) << 32)` | One 8-byte store sets rc=1 / tag=Str / flags=0 |
| `STR_POOL_PAYLOAD` = 16, `STR_POOL_SLOTS` = 32 | Pool size class |
| `SUBSTR_HDR_SIZE` = 32 | header + len + parent + off |
| `FLAG_STATIC_LITERAL` bit | `.rodata` Str blocks; refcount ops no-op |

## Spec coverage

Implements the following ECMAScript surface entirely in this crate:

- `String.prototype.length` / `[i]` / `slice` / `substring` / `substr` /
  `charAt` / `charCodeAt` / `codePointAt`
- `startsWith` / `endsWith` / `indexOf` / `includes` / `lastIndexOf`
- `concat` / `repeat` / `padStart` / `padEnd` / `trim` / `trimStart` /
  `trimEnd` / `toUpperCase` / `toLowerCase`
- `split(sep)` / `split(/regex/)` (regex variant delegates to
  `torajs-regex`)
- `String.prototype.valueOf` / `toString`
- `Symbol(...)` factory + `Symbol.prototype.{description, toString, valueOf}`
- Static literal Str sharing (`.rodata` Str blocks, refcount-free)
- `JSON.stringify` of string body + `JSON.parse` of string body

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
