# torajs-regex

[![Crates.io](https://img.shields.io/crates/v/torajs-regex?style=flat-square&logo=rust)](https://crates.io/crates/torajs-regex)
[![docs.rs](https://img.shields.io/docsrs/torajs-regex?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-regex)
[![License](https://img.shields.io/crates/l/torajs-regex?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-regex?style=flat-square)](https://crates.io/crates/torajs-regex)

ECMAScript-flavored regex engine for the [torajs] AOT TypeScript
runtime: parser (recursive-descent) → Thompson-NFA compiler → Pike-VM
matcher. 0 Cargo deps. Full surface: lookahead / lookbehind / backrefs
/ named captures / `u`-flag (UTF-8 + curated UCD Letter / Number
tables).

Extracted from `runtime_regex.c` (3059 LOC of C) — the **largest
single C runtime file** that Phase 1 nuked — as **P6.2** sub-step
sequence (commits `2aced81` through `e481e99`, 2026-05-24). 12
source modules, ~4.4 KLOC pure Rust.

## Implementation choices

| Aspect | Choice | Source |
| --- | --- | --- |
| Parsing | Recursive-descent over pattern bytes | Standard |
| Compilation | Thompson NFA | [Cox 2007 part 1](https://swtch.com/~rsc/regexp/regexp1.html) |
| Matching | Pike VM (NFA simulator) | [Cox 2007 part 2](https://swtch.com/~rsc/regexp/regexp2.html) |
| Backrefs | Pike VM with capture-aware fork | Standard |
| Lookahead/Lookbehind | Reverse-compile + recursive sub-match | Standard |
| Unicode | Curated UCD Letter / Number tables (no full ICU) | `torajs-ucd` |

## Surface (full ES regex API)

- Construction: `/pat/flags` literal + `new RegExp(pat, flags)`
- `regex.test(s)` / `regex.exec(s)` / `regex.matchAll(s)` (yields iterator)
- `s.match(/pat/)` / `s.matchAll(/pat/)`
- `s.replace(/pat/, repl)` / `s.replaceAll(/pat/, repl)` (literal + fn variants)
- `s.split(/pat/[, limit])`
- `s.search(/pat/)`
- `regex.lastIndex` (read + write — sticky / global iteration anchor)
- Named captures: `(?<name>...)` + `match.groups.name`
- `u`-flag: UTF-8 char-level matching + Unicode property escapes
- Flags: `g` / `i` / `m` / `s` / `u` / `y` (sticky)

## Modules (12 files, ~4.4 KLOC)

| Module | Purpose |
| --- | --- |
| `lib.rs` | Re-exports + extern boundary |
| `regex/mod.rs` | Top-level RegExp heap layout + compile/drop |
| `regex/parse.rs` | Recursive-descent parser → AST |
| `regex/compile.rs` | AST → Thompson NFA |
| `regex/match_op.rs` | exec / test / matchAll / lastIndex glue |
| `regex/match_all.rs` | matchAll iterator |
| `regex/match_replace.rs` | Replace literal + fn variants |
| `regex/match_split.rs` | Split into Array<Substr> |
| `regex/charclass.rs` | Character class + `[...]` set ops |
| `regex/unicode.rs` | `u`-flag UTF-8 path + UCD escape resolution |
| `regex/backref.rs` | Pike VM with capture-aware fork |
| `regex/lookaround.rs` | Lookahead / lookbehind reverse-match |

## What's NOT in scope (v0.1.0)

- **Atomic groups** `(?>...)`: not in ECMAScript today; planned post-
  Stage 3.
- **Possessive quantifiers** `*+` `?+` `++`: not in ECMAScript.
- **Full ICU Unicode property names**: only the most-used subset
  is in `torajs-ucd`. Property names like `\p{L}` / `\p{N}` work;
  exotic names fall back to error.
- **JIT compilation of NFA → native code**: V8 has it; we don't —
  yet. Pike VM is the algorithm choice for v0.1.

## License

Dual-licensed: Apache-2.0 / MIT — see [LICENSE-APACHE](LICENSE-APACHE)
+ [LICENSE-MIT](LICENSE-MIT).

[torajs]: https://torajs.com
