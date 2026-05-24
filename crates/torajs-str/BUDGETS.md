# torajs-str performance budgets

`torajs-str` sits on the **hottest** path in the workspace — every
string-touching op (Array element equality, Map key compare,
JSON.parse, console.log) ends up here. Per-op micro-budgets matter.

Budgets here are documentary (no perf-gate tests because cargo
test against the rlib doesn't link the full IR-emit toolchain
needed for end-to-end timing); `benches/str.rs` reports the
actual numbers via criterion + the end-to-end bench corpus
(csv-rebuild / csv-trim / split-only) validates the integrated
behavior.

## Path taxonomy

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `alloc::__torajs_str_alloc_pooled` | **Hot** | Per Str construction. Small-Str pool hit-rate is ~95% for the bench corpus. |
| `alloc::__torajs_str_free` | **Hot** | Pairs with alloc; pool-aware free. |
| `pool::pop / push` | **Hot** | Lock-free LIFO; ~5 ns per op. |
| `eq::__torajs_str_eq` | **Hot** | Per `===` / `!==` on string operands; per Array.includes element; per Map key compare. |
| `lookup::__torajs_str_char_code_at` | **Hot** | Per `s.charCodeAt(i)` call. Bench corpus runs ~600k/sec on csv-rebuild. |
| `slice::__torajs_str_slice` | **Warm** | Per `s.slice(a, b)` call. Csv-trim runs ~700k/sec. |
| `concat::__torajs_str_concat` | **Warm** | Per `a + b` for Str. Csv-rebuild's `parts[i] + "|"` runs 600k/sec. |
| `transform/case` (toUpper / toLower) | Warm | ASCII fast path + general unicode-aware path. |
| `json/json_parse` | Cold | One alloc per JSON parse / stringify of a string body. |
| `print` | Cold | Once per console.log call. |

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `str_alloc_free_8byte-100k` | ≤ 2 ms | ~0.6 ms | 3× | Pool hit, 100% reuse. ~6 ns / alloc-free pair. |
| `str_eq_48byte-100k` | ≤ 1 ms | ~0.3 ms | 3× | Length check + memcmp 48 bytes. ~3 ns / op. |
| `str_slice_64byte-100k` | ≤ 5 ms | ~2 ms | 2.5× | Alloc 30-byte block + memcpy 30 bytes. ~20 ns / op. |

## Integrated bench validation

The integrated bench corpus (run via `cargo run -p bench-harness --release -- run`)
exercises this crate in the realistic combinations:

- `csv-rebuild-100k`: 6 × concat per iteration → 600k concat/run; targets 7-8 ms total.
- `csv-trim-100k`: 7 × split + 7 × trim per iteration → 700k slice/run; targets 7-8 ms total.
- `split-only-100k`: 1 × split per iteration → 100k split/run; targets 5 ms total.

Per `bench/results/2026-05-24-mini-9b7740c.json` (Phase 1 closed baseline):

- csv-rebuild-100k torajs: 7.40 ms (rust: 18.96 ms; bun-aot: 21.89 ms — torajs **2.56× rust / 2.96× bun**)
- csv-trim-100k    torajs: 7.44 ms (rust: 7.10 ms; bun-aot: 21.95 ms — torajs neck-and-neck with rust, **2.95× bun**)
- split-only-100k  torajs: 4.82 ms (rust: 8.75 ms; bun-aot: 10.28 ms — torajs **1.82× rust / 2.13× bun**)

## Allocation philosophy

- **Small Str (payload ≤ 16 B)**: pool hit. ~5 ns alloc / ~5 ns free.
- **Medium Str (16 B < payload ≤ 64 B)**: `std::alloc::alloc(Layout::array)`.
  ~30 ns alloc / ~30 ns free.
- **Static literal Str**: zero-cost. The `.rodata` block carries
  the `FLAG_STATIC_LITERAL` bit; alloc returns it as-is, free no-ops,
  refcount ops no-op. Used pervasively for string literals in user
  code (every `"hello"` in TS source is a static literal).

## What's NOT budgeted

- **Regex split / replace**: those delegate to `torajs-regex`; the
  per-op cost belongs to that crate's budget.
- **Full unicode case conversion**: the `transform/case` ASCII fast
  path covers the common case; general unicode path uses `torajs-ucd`
  + costs more (~5× per byte).
- **JSON.parse of large objects / arrays**: that's `torajs-anyvalue` /
  `torajs-dynobj` / `torajs-arr` territory; only the string-body
  parse + escape decode lives here.
- **Cross-tier link via `extern "C"`**: this crate's externs are
  resolved at `tr build` link time. The bench numbers above are
  via the rlib path; the staticlib path adds a `bl + ret` overhead
  that LTO removes for fat-LTO builds.
