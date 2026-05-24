# Changelog

All notable changes to `torajs-dynobj` are documented in this file.
Format per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-05-23

Initial scaffold (P4.2 ship sequence — `f90866f` through `17397a5`,
2026-05-23): scaffold + dynobj_alloc + probe/hash/eq + get/set +
define + delete/has + drop. 11 modules across set / get / has /
delete / define / iter / attrs / alloc / drop / layout / lib.

### Polished (2026-05-25)

LICENSE-MIT + LICENSE-APACHE; README with algorithm choices + module
layout + scope delimiter; BUDGETS.md (set/get/delete latency); benches/
dynobj.rs placeholder; Cargo.toml dev-dep.
