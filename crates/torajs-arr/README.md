# torajs-arr

[![Crates.io](https://img.shields.io/crates/v/torajs-arr?style=flat-square&logo=rust)](https://crates.io/crates/torajs-arr)
[![docs.rs](https://img.shields.io/docsrs/torajs-arr?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-arr)
[![License](https://img.shields.io/crates/l/torajs-arr?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-arr?style=flat-square)](https://crates.io/crates/torajs-arr)

`Array<T>` + `Array<Any>` substrate for the [torajs] AOT TypeScript
runtime — push / pop / shift / unshift / iter / slice / concat / map
/ filter / reduce / sort / etc. over a refcounted pool-aware backing
block. 0 Cargo deps.

Extracted from `runtime_str.c`'s array family as **P4.1** sub-step
sequence (commits across 2026-05-23 / 24). 16 source modules covering
the full JS `Array.prototype` surface.

## Heap layout

```text
Array<T>     = [header:8][len:8][cap:4][head:4][slots:cap*8]
Array<Any>   = same shape with 16-byte slots (tag+value)
```

The 4-byte `head_offset` (T-13.5) enables **O(1) `shift()`** by
sliding the logical-start pointer instead of memmoving the rest.
Compaction triggers on push when `head + len >= cap`.

## Surface (16 modules)

| Module | LOC | Purpose |
| --- | ---: | --- |
| `alloc.rs` | ~120 | Pool-aware Array alloc |
| `drop.rs` | ~150 | Drop + per-slot value drop dispatch |
| `grow.rs` | ~210 | push + reserve + shift |
| `ops.rs` | ~80 | push_unchecked + extend_unchecked |
| `iter.rs` | ~400 | ArrIter family (forEach + values + keys + entries) |
| `slice.rs` | ~120 | slice + spread |
| `transform.rs` | ~450 | map / filter / reduce / reduceRight |
| `sort.rs` | ~300 | sort + reverse |
| `concat.rs` | ~100 | concat (varargs) |
| `join.rs` | ~120 | join(sep) |
| `find.rs` | ~150 | find / findIndex / findLast / findLastIndex / includes |
| `index_of.rs` | ~100 | indexOf / lastIndexOf |
| `any.rs` | ~120 | Array<Any> tag-aware ops |
| `from_string.rs` | ~80 | Array.from(string) |
| `print.rs` | ~80 | Array.toString / debug print |
| `arrprops.rs` | ~80 | Side-table for `arr.someProperty = v` |

## Performance highlights

This crate is on the **single hottest hot path** in the runtime —
every JS array op flows through here. Bench-corpus numbers from
the Phase 1 closed baseline:

| Case | torajs | rust | bun-aot | vs bun |
| --- | ---: | ---: | ---: | ---: |
| array-sum-1m | 12.7 ms | 13.7 ms | 49.4 ms | **4.15×** |
| array-map-1m | 22.6 ms | 23.2 ms | 57.8 ms | **2.59×** |
| stack-pop-1m | 2.42 ms | 2.69 ms | 15.0 ms | **6.21×** |
| fifo-queue-100k | 1.49 ms | 1.50 ms | 10.1 ms | **6.80×** |

The B1b / B4-shift / B4-push-unchecked IR-restore work in 2026-05-24
session brought `array-sum-1m` from 21.5 ms to 12.7 ms (-41%) by
restoring the inkwell-emitted alwaysinline path for push / shift /
push_unchecked.

## License

Dual-licensed: Apache-2.0 / MIT — see [LICENSE-APACHE](LICENSE-APACHE)
+ [LICENSE-MIT](LICENSE-MIT).

[torajs]: https://torajs.com
