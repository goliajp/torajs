# Changelog — torajs-arr

Per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-05-23

Initial release: alloc / drop, push_unchecked / reserve / push /
shift, arrprops side-table, iterator family, slice, transform
(map / filter / reduce / reduceRight), sort, concat, join,
find / index_of, Array<Any> tag-aware ops, `Array.from(string)`,
and print/debug support across 16 modules.

### Changed

- Restored inlined codegen paths for `push` / `shift` /
  `push_unchecked`, improving throughput on array-heavy workloads.
