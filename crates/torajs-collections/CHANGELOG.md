# Changelog — torajs-collections

Per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-05-24

Initial ship via P4.3 sub-step sequence (commits `fbb052b` through
`ab95e60`, 2026-05-24): scaffold + map_create → hash/probe + size/
has/get → map_set + slot_insert/rehash → delete/clear → map_drop →
MapIter family → ArrIter migration + final nuke of runtime_map.c.
11 modules, ~1.5 KLOC.

### Polished (2026-05-25)

LICENSE-MIT + LICENSE-APACHE; README with two-array design rationale +
SameValueZero notes + modules table; BUDGETS.md per-op + memory
overhead; benches/collections.rs placeholder.
