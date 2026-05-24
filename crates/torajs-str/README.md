# torajs-str

[![Crates.io](https://img.shields.io/crates/v/torajs-str?style=flat-square&logo=rust)](https://crates.io/crates/torajs-str)
[![docs.rs](https://img.shields.io/docsrs/torajs-str?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-str)
[![License](https://img.shields.io/crates/l/torajs-str?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-str?style=flat-square)](https://crates.io/crates/torajs-str)

Layer-2 substrate of the [torajs] AOT TypeScript runtime: the `Str` +
`Substr` heap types and ALL spec-mandated string operations
(`String.prototype.*`, `JSON.parse` of string body, symbol family,
print). Pure Rust (`std`-only); 2 Cargo deps (`torajs-rc` for the
universal heap header + refcount, `torajs-abort` to keep the panic
infra out of the user binary).

Extracted from `runtime_str.c`'s ~5 KLOC of string ops across the
P3.1 ship sequence (P3.1-a through P7.b — 2026-05-23 to 2026-05-24).
By P7.i-closer (`21a0feb`) all string surface lived in this crate; by
A.1 (this session) even the comment-only `runtime_str.c` stub was
deleted.

## Layout

```text
Str    = [header:8][len:8][bytes:N]           prefix 16
Substr = [header:8][len:8][parent:8][off:8]   prefix 32
```

The 8-byte universal header (`refcount:u32 + type_tag:u16 + flags:u16`)
is supplied by `torajs-rc`; the rest is `torajs-str`'s. `STR_HDR_SIZE`
= 16; `SUBSTR_HDR_SIZE` = 32. Bytes are not NUL-terminated; the `len`
field is authoritative.

## Modules (18 source files across 4 sub-directories)

| Module | LOC | Purpose |
| --- | ---: | --- |
| `layout.rs` | ~80 | ABI offset / size constants + packed header init |
| `alloc.rs` | ~250 | Pool-aware `alloc` + `free_pool_aware` + `__torajs_str_*` extern wrappers |
| `pool.rs` | ~150 | Small-Str LIFO pool (32 slots × 16-byte payload) |
| `concat.rs` | ~120 | `a + b` for Str operands; single alloc + 2 copies |
| `slice.rs` | ~220 | `String.prototype.slice / substring / substr` with ES negative-index normalization |
| `lookup.rs` | ~250 | `charCodeAt` / `startsWith` / `endsWith` / `indexOf` / `includes` |
| `eq.rs` | ~180 | `===` / `!==` byte-equality, with FLAG_STATIC_LITERAL fast path |
| `print.rs` | ~140 | stdout / stderr Str + Substr printing (locked + ordered with print_i64 etc.) |
| `literals.rs` | ~200 | static .rodata Str-shaped globals + their FLAG_STATIC_LITERAL handling |
| `to_number.rs` | ~250 | `Number(s)` ES §7.1.4 conversion |
| `json.rs` | ~320 | `JSON.stringify` string body (quote escape, octal escape, JSON-specific escapes) |
| `json_parse/` | ~700 (3 files) | `JSON.parse` of string body (escape decode, unicode escape, surrogate pair handling) |
| `transform/` | ~700 (4 files) | `toUpperCase` / `toLowerCase` / `repeat` / `padStart` / `padEnd` / `trim` |
| `split/` | ~400 (3 files) | `split(sep)` + `split(/regex/)` → `Array<Substr>` |
| `substr.rs` | ~530 | `Substr` heap type + `SubstrBlock` + pool ops |
| `substr_methods.rs` | ~400 | Substr-typed `charCodeAt` / `slice` / etc. (Phase B view-aware dispatch) |
| `symbol.rs` | ~150 | `Symbol` family (description / toString / valueOf) |

Total: ~4.7 KLOC pure Rust replacing ~5 KLOC of `runtime_str.c`.

## ABI invariants (must not change)

| Symbol | Stable since | Notes |
| --- | --- | --- |
| `STR_HDR_SIZE` = 16 | P3.1-a | Universal 8-byte header + 8-byte len |
| `STR_LEN_OFF` = 8 | P3.1-a | u64 byte-length |
| `STR_DATA_OFF` = 16 | P3.1-a | First payload byte |
| Packed init: `1u64 \| ((Tag::Str as u64) << 32)` | P3.1-a | One 8-byte store sets rc=1 / tag=Str / flags=0 |
| `STR_POOL_PAYLOAD` = 16, `STR_POOL_SLOTS` = 32 | P3.1-a | Pool size class |
| `SUBSTR_HDR_SIZE` = 32 | P3.1-b | header + len + parent + off |
| `FLAG_STATIC_LITERAL` bit | P3.1-a | `.rodata` Str blocks; refcount ops no-op |

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
