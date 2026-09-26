# torajs-arr

[![Crates.io](https://img.shields.io/crates/v/torajs-arr?style=flat-square&logo=rust)](https://crates.io/crates/torajs-arr)
[![docs.rs](https://img.shields.io/docsrs/torajs-arr?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-arr)
[![License](https://img.shields.io/crates/l/torajs-arr?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-arr?style=flat-square)](https://crates.io/crates/torajs-arr)

`Array<T>` + `Array<Any>` substrate for the [torajs] AOT TypeScript
runtime — push / pop / shift / unshift / iter / slice / concat / map
/ filter / reduce / sort / etc. over a refcounted pool-aware backing
block. 0 Cargo deps. Covers the full JS `Array.prototype` surface
across 16 source modules.

## Heap layout

```text
Array<T>     = [header:8][len:8][cap:4][head:4][slots:cap*8]
Array<Any>   = same shape with 16-byte slots (tag+value)
```

The 4-byte `head_offset` enables **O(1) `shift()`** by sliding the
logical-start pointer instead of memmoving the rest. Compaction
triggers on push when `head + len >= cap`.

## Surface (16 modules)

| Module | Purpose |
| --- | --- |
| `alloc.rs` | Pool-aware Array alloc |
| `drop.rs` | Drop + per-slot value drop dispatch |
| `grow.rs` | push + reserve + shift |
| `ops.rs` | push_unchecked + extend_unchecked |
| `iter.rs` | ArrIter family (forEach + values + keys + entries) |
| `slice.rs` | slice + spread |
| `transform.rs` | map / filter / reduce / reduceRight |
| `sort.rs` | sort + reverse |
| `concat.rs` | concat (varargs) |
| `join.rs` | join(sep) |
| `find.rs` | find / findIndex / findLast / findLastIndex / includes |
| `index_of.rs` | indexOf / lastIndexOf |
| `any.rs` | Array<Any> tag-aware ops |
| `from_string.rs` | Array.from(string) |
| `print.rs` | Array.toString / debug print |
| `arrprops.rs` | Side-table for `arr.someProperty = v` |

## License

Dual-licensed: Apache-2.0 / MIT — see [LICENSE-APACHE](LICENSE-APACHE)
+ [LICENSE-MIT](LICENSE-MIT).

[torajs]: https://torajs.com
