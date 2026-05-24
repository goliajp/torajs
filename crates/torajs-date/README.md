# torajs-date

[![Crates.io](https://img.shields.io/crates/v/torajs-date?style=flat-square&logo=rust)](https://crates.io/crates/torajs-date)
[![docs.rs](https://img.shields.io/docsrs/torajs-date?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-date)
[![License](https://img.shields.io/crates/l/torajs-date?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-date?style=flat-square)](https://crates.io/crates/torajs-date)

JavaScript `Date` class substrate for the [torajs] AOT TypeScript
runtime. 0 Cargo deps (uses libc `localtime_r` / `mktime` for
timezone-aware accessors; Howard Hinnant `civil_from_days` /
`days_from_civil` for proleptic Gregorian conversions).

Extracted from `runtime_date.c` (~590 LOC of C) as **P6.4**
(commit `243e665`, 2026-05-24).

## Surface

Implements the full ECMAScript `Date` surface that maps to a 16-byte
heap object (header + i64 ms-since-epoch):

- `new Date()`, `new Date(ms)`, `new Date(yr, mo, d, h, m, s, ms)`,
  `new Date(isoStr)` — constructors via `__torajs_date_new_*`
- `Date.now()`, `Date.UTC(...)`, `Date.parse(...)`
- UTC getters: `getUTCFullYear` / `getUTCMonth` / `getUTCDate` /
  `getUTCHours` / `getUTCMinutes` / `getUTCSeconds` /
  `getUTCMilliseconds` / `getUTCDay`
- Local-time getters: same but without `UTC` (uses libc localtime_r)
- `getYear` (Annex B legacy)
- `setTime` / `setFullYear` / `setHours` / ... — full setter family
- `toISOString` (ISO 8601 with Z timezone)
- `toGMTString` (RFC 7231 IMF-fixdate format)

## Module layout

| Module | LOC | Purpose |
| --- | ---: | --- |
| `layout.rs` | ~80 | Heap layout (header + ms slot) + alloc |
| `tm.rs` | ~120 | Decomposed time-struct (year/mo/day/...) |
| `civil.rs` | ~250 | Howard Hinnant civil_from_days + days_from_civil + leap-year arithmetic |
| `getters.rs` | ~250 | All getX / getUTCX accessors |
| `parse.rs` | ~150 | ISO 8601 string parser (RFC 3339 subset) |
| `api.rs` | ~140 | Constructor + setter + toString externs |

## ABI invariants

- `Date` heap block = 16 bytes (universal header 8 + i64 ms slot 8)
- `Tag::Date` type_tag for `__torajs_value_drop_heap` dispatch
- ms-since-epoch is **signed i64** — supports pre-1970 dates and
  far-future dates within ±290M years
- All conversions use proleptic Gregorian (no Julian fallback for
  pre-1582 dates)

## What's NOT in scope (v0.1.0)

- **Intl.DateTimeFormat**: per ES-Intl spec — separate crate.
- **Timezone other than UTC + local**: explicit-timezone parsing
  (e.g. `"2024-01-01T00:00:00+09:00"`) is supported by ISO parse,
  but getter / setter are only UTC + local.
- **toLocaleString variants**: out of scope until Intl lands.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
