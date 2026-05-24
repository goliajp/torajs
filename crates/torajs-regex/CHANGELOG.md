# Changelog — torajs-regex

Per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-05-24

Initial ship via P6.2 sub-step sequence (commits `2aced81` /
`31e854a` / `9de5e10` / `1f73664` / `e481e99`, 2026-05-24).
3059 LOC of C → 4.4 KLOC Rust in 12 modules. Full ECMAScript
regex surface (test / exec / replace / split / matchAll /
lookahead+lookbehind / backref / named captures / `u` flag).

### Polished (2026-05-25)

LICENSE-MIT + LICENSE-APACHE; README with implementation table
(parser → Thompson NFA → Pike VM) + full surface list + module
table + scope delimiter; BUDGETS.md per-op latency; benches/regex.rs
placeholder.
