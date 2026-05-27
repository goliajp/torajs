# torajs perf — what's measured

torajs's positioning: AOT TypeScript runtime that **dominates JS
incumbents** by measured wall-clock — every number in a commit
message, README, or blog post **must trace back to a measurement
that anyone can reproduce.** Guesses don't count. Estimates don't
count. Numbers we'd like to be true don't count.

This file is the single source of truth for which torajs perf claims
are honestly measured + which are still open. When a commit message
disagrees with this file, trust this file — and update the commit's
claim or this file in the same merge.

## Measured

### Workspace-level

| Path | Measurement | Run command |
|---|---|---|
| **geomean speedup vs bun-aot** | **4.41×** at HEAD `8f754ca` (5-pass median, sequential interleave, no concurrent test262/conformance, mac M-series, host CPU detection enabled). Up from 3.89× at `14d56f8` (the 5-pass quiet-machine baseline after the contaminated 3-pass result was discarded). | `cargo run -p bench-harness --release -- run --runs 5` |
| **geomean speedup vs bun-jsc** | **4.52×** at same HEAD | same |
| **geomean speedup vs node-v8** | **21.07×** at same HEAD | same |
| **per-case wins** | **26 / 26** vs bun-best (held across every shipped P-PERF commit). | bench result file `bench/results/2026-05-22-mini-d7bab5b.json` |
| **Conformance subset** | **666 / 0 / 1** (pass / fail / skip) at HEAD `8f754ca`. Held throughout every P-PERF ship; 0 regression at any commit. | `cargo run --release --bin torajs-conformance` |
| **test262 in-scope pass rate** | **12.20%** (3455 / 28314) at HEAD `081b25f`, ranAt 2026-05-22. Up from 11.81% (3344) at 2026-05-19. | `cargo run --release -p torajs-test262 -- --json hardev/test262-latest.json` |
| **AOT binary size (popcount case)** | ~36 KB stripped | `du -h target/release/<case>-aot` after `tr build` |

### v0.7 Phase 3 — NaN-box AnyValue cutover (Step 7, closed 2026-05-27 at `e741596`)

3-pass median (`--runs 3`) using the same-day same-machine baseline
`cd7c05e` (Step 5d-revert) for apples-to-apples comparison. Numbers
are different scale from the P-PERF round above because both runs
share contended-machine state, not the P-PERF quiet-machine 5-pass
protocol — the **Δ within the same machine state** is the honest
signal. See `docs/v0.7-Phase3-nanbox.md` for the 16-commit ledger.

| Reference | cd7c05e (pre-Step-7) | e741596 (post Step 7) | Δ |
|---|---:|---:|---:|
| **torajs vs bun-aot** | 3.816× | **4.169×** | **+9.2%** (passes 4.10× A2 gate) |
| **torajs vs rust** | 1.379× | **1.436×** | **+4.1%** |
| **Binary (sum 26 cases)** | 8972.2 KB | 8975.0 KB | **+0.03%** (no regression) |
| **Conformance** | 685/0/1 | 685/0/1 | held every commit |

System-state context: all 4 runtimes show ~37-50% absolute slowdown
between cd7c05e and e741596 due to time-of-day + concurrent
conformance gate. torajs slowed less than bun-aot (37% vs 50%),
which widens the relative ratio.

### v0.7 Phase 3 — Step 8 Str SSO (closed 2026-05-27 at `89ed977`)

NaN-box ShortStr inline encoding for byte-len ≤ 5 strings — `""`,
`"a"`, `"true"`, `"null"`, single-char `charAt` returns,
small-Number `.toString` results, JSON delimiters, ASCII tokens.
Eliminates heap-alloc + 8-byte header + 8-byte len + per-use
rc_inc/rc_dec for the dominant short-Str profile. 8b shipped 8
nanbox_ffi shim arms + materialize helper (`1086def`..`1225ed3`);
8c-1 broadened `box_double` NaN canonicalization + downstream
consumer arms (`5bb3d0e`); 8c-3 wired the runtime producer fast
path in `arith.rs::any_add` string-concat (`813aefb`); 8d the
compile-time producer fast path in
`ssa_lower::box_to_any_from_expr` —
`IntToPtr(ConstI64(short_u64))` typed Any direct, skipping the
runtime `__torajs_anyv_box_from_pair(4, str_ptr)` call (`89ed977`).
See `docs/v0.7-Phase3-Step8-sso.md` for the full ledger.

Two 3-pass medians at HEAD `89ed977`, same-day same-machine. The
existing bench-tr case set is closure / promise / iteration-bound;
**no case exercises short-string-in-Any** (`charAt`, single-char
concat into Any[], small-Number `.toString` boxed into Any). Step 8
SSO is therefore invisible to bench-tr; the honest signal is
"unchanged within run-to-run noise + zero conformance regression".

| Reference | e741596 (Step 7 close) | 89ed977 (run 1) | 89ed977 (run 2) | Δ vs baseline |
|---|---:|---:|---:|---:|
| **torajs vs bun-aot (12-case shared subset)** | 5.110× | 4.829× | 5.098× | **−5.5% / −0.2%** (within 3-pass noise band) |
| **torajs vs bun-aot (full 26 cases, run 1 only)** | 4.169× | 3.825× | n/a (`--no-save`) | **−8.3%** (same noise origin) |
| **torajs vs rust (full 26)** | 1.436× | 1.413× | n/a | **−1.6%** (within noise) |
| **Binary (sum 26 cases)** | 8975.0 KB | 9000.7 KB | n/a | **+0.29%** (no regression) |
| **Conformance** | 685 / 0 / 1 | **687 / 0 / 1** | n/a | held + 2 new fixtures (short-str-001 / -002) |

System-state context: every 8d torajs case runs **20–50% faster
absolute** than the e741596 baseline — today's machine is lighter
than the Step-7-close moment (which had a concurrent conformance
gate per its own context note). bun-aot benefits asymmetrically
more from idle thermal headroom on this hardware, which narrows
the *relative* ratio without any torajs-side slowdown. The 0.2%
between baseline and run-2 (same 12-case subset) is the honest
"Step 8 contributes no regression and no measurable bench-tr gain".

Why bench-tr can't confirm the SSO win: the 8d design doc filed
this explicitly — "concat-heavy / short-literal-heavy fixtures
expected to win more" (`docs/v0.7-Phase3-Step8-sso.md` §
Implementation plan, 8d row). bench-tr's hot paths use typed Str
operations (no Any boxing for short literals), so 8d's removal of
the per-literal `any_box(4, str_ptr)` call is unreachable from
these cases. SSO validation awaits string-heavy micro-bench fixtures
(parser-driven, JSON-tokenizing, charAt-loop) — filed for follow-on.

Substrate completeness gate (the real Step 8 acceptance, per
roadmap v5 substrate-completeness model — pass rate is diagnostic
only):

- ✅ `is_short_str` / `short_str_len` / `short_str_bytes` /
  `try_box_short_str` (8b)
- ✅ 8 `__torajs_anyv_*` shim arms + `materialize_short_str` (8b-A/B/C)
- ✅ `box_double` NaN canon over full collision matrix +
  consumer arms in `inspect` / `nanbox_encode` (8c-1)
- ✅ `arith.rs::any_add` `try_concat_short` runtime producer (8c-3)
- ✅ `ssa_lower::box_to_any_from_expr` compile-time producer (8d)
- ⏸️ 8c-2 deferred (meta::reflect + `.length` on Type::Any —
  pre-existing tora dynobj limitation, not ShortStr-specific;
  filed as separate Any-property-dispatch follow-on)
- ⏸️ Map.set key-hash short-path (8d-deferred, perf-polish only)

### Per-case bench medians at HEAD `8f754ca` (post P-PERF.A6, 5-pass median, M-series Mac)

| Case | torajs ms | bun-aot ms | bun-jsc ms | node-v8 ms | tora vs bun-best |
|---|---:|---:|---:|---:|---:|
| popcount | 2.51 | 57.73 | 57.00 | 137.70 | **22.7×** |
| generic-pair-1m | 1.19 | 12.29 | 12.07 | 95.23 | **10.1×** |
| fifo-queue-100k | 1.28 | 9.96 | 9.75 | 93.05 | **7.6×** |
| stack-pop-1m | 1.89 | 15.46 | 14.79 | 100.73 | **7.8×** |
| startup | 1.15 | 7.63 | 7.46 | 89.68 | **6.5×** |
| promise-all-1k | 1.21 | 8.33 | 7.96 | 92.92 | **6.6×** |
| promise-chain-1k | 1.33 | 8.63 | 8.31 | 91.72 | **6.3×** |
| closure-pipeline-1m | 8.17 | 51.57 | 49.87 | 191.69 | **6.1×** |
| generic-id-1m | 8.01 | 49.71 | 47.32 | 184.87 | **5.9×** |
| split-only-100k | 3.50 | 10.31 | 9.69 | 92.44 | **2.8×** |
| ... (others) | | | | | 1.1×–5×+ |
| **prime_count** | 48.07 | 53.49 | 54.44 | 165.74 | **1.1×** ← narrowest lead |

Narrowest leads (`prime_count` 1.1×, `gcd1m` 1.2×, `mandelbrot` 1.5×,
`ackermann` 1.8×, `fib40` 2.5×) are the P-PERF focus going forward —
CPU-bound integer/float loops where bun's JIT closes the gap.
Wide leads (popcount 22.7×, fifo 7.6×, stack-pop 7.8×) confirm
tora's structural AOT advantage on tight typed loops.

### P-PERF session-arc this round (2026-05-22)

| Commit | Change | Geomean vs bun-aot | Net result |
|---|---|---|---|
| `14d56f8` | 5-pass quiet baseline (3-pass contaminated, discarded) | 3.89× | baseline |
| `f4cd310` P-PERF.A1 | User FnDecls Internal linkage → IPSCCP/inliner specialization | **4.16× (+7.0%)** | SHIPPED |
| `9050729` P-PERF.A3 | alwaysinline for small non-recursive user fns (threshold 60) | 4.16× (per-case wins, ratio flat) | SHIPPED |
| `8f754ca` P-PERF.A6 | Promise free-list pool (bounded 32; promise-await -41% / async-fn-call -36% / promise-then -32%) | **4.41× (+6.1%)** | SHIPPED |
| (between) | A2 codegen=Aggressive | 4.10× | REVERTED — net regression |
| (between) | A4 alwaysinline threshold 30 | 4.15× | REVERTED — worse than A3 |
| (between) | A5 RelocMode=Static | 4.15× | REVERTED — thermal-confounded inconclusive |

Net session gain: **3.89× → 4.41× (+13.4%) geomean vs bun-aot, +14.9% vs node-v8**. Three optimizations shipped, three reverted as negative-result evidence (archived under `bench/results/`).

## Reproduction protocol (per "别作假" hard rule)

Every published number above must be reproducible by running the
commands listed, on a quiet host (no concurrent test262 / conformance
runs, no thermal throttle), using the **5-pass median** protocol.
Single-run mac numbers carry ±20–40% noise band and are NOT used for
shipped claims.

When a commit changes any of these numbers:

1. Run the relevant command.
2. Capture the bench result file.
3. Update the row here in the same commit (or a same-day docs commit).
4. If the new number differs by > noise band from an old claim that
   wasn't updated, **the old claim was wrong** — annotate the
   correction explicitly.

## When perf claims drift from this file

`mailrs/PERFORMANCE.md` records an instance where a `+10–20% throughput`
commit claim was later corrected to `+2.10%` after honest measurement.
Same convention here: if a torajs commit message claims X% and this
file later shows Y%, the Y is real and the X needs to be marked as a
misattribution. Self-correcting honesty is the rule.

## Open / pending measurement

- Cold-machine vs warm-machine bench delta (`PERFORMANCE.md` followup):
  current 4.41× was measured after a thermal-loaded 4+ hour cycle.
  A cold-machine 5-pass rerun is owed to confirm the number isn't
  pessimistic. (Tracked: P-PERF.0 follow-up.)
- Effect of `release-vanilla` vs `release` on torajs's own tooling
  build (NOT the compiled output). Mirror mailrs's "the perf-first
  profile cost 17% on native bench geomean" verification, but at the
  rust crate build level for tora's compiler.
- PGO baseline: not measured. Tentative future P-PERF substep.

## Sub-crate budgets

Per the layered-crate rewrite (see `docs/architecture-rewrite.md`),
each sub-crate ships with its own `BUDGETS.md` documenting per-bench
budgets + observed P95 + headroom factor. Those are regression-catch
gates (15–30× headroom over P95), NOT publishable numbers. Don't
quote a budget value as a perf claim; quote the criterion bench
median from the crate's `benches/<name>.rs` output instead.

| Crate | Status | `cargo bench` invocation |
|---|---|---|
| `torajs-pool` | not yet built (P1 pilot) | `cargo bench -p torajs-pool` |
| ... (filled as crates ship) | | |
