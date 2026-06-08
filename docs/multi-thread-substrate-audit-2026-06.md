# Multi-thread substrate audit — 2026-06-08

> Output of P3 of the multi-thread vision rollout (see
> `.claude/vision.md` 三-1, `rules/torajs-design-principles.md` §6,
> `docs/roadmap.md` axis D + P16 placeholder).
>
> Scope: grep-driven audit of single-mutator assumptions across the
> Rust workspace. Findings classified by remediation timing. **No
> retrofit commits in P3** — current substrate (incl. chunk 9d splice
> + P2 RC emit hook) is axis-D compatible; findings here are the
> P16+ implementation backlog. Future commits referencing these
> sites cite this audit by section letter (Group A.1, etc.).
>
> Audit method:
>
> ```bash
> grep -rnE '^[[:space:]]*static mut\b|pub static mut\b' --include='*.rs' crates/
> grep -rnE 'static.*Mutex<|static.*RwLock<'             --include='*.rs' crates/
> grep -rnE 'OnceLock<Mutex|OnceLock<RwLock'             --include='*.rs' crates/
> grep -rnE 'lazy_static!|once_cell::sync::Lazy.*Mutex'  --include='*.rs' crates/
> grep -rnE '__thread\b|#\[thread_local\]|thread_local!' --include='*.rs' crates/
> ```
>
> HEAD `e12dc80` (P2 ship). New mutator sites must not appear past
> this audit without explicit P16+ design or grandfather note.

---

## Group A — production hot-path single-mutator sites (P16 retrofit targets)

These are the substrate sites that **need real changes** in the
post-v1.0 P16+ multi-thread trunk. Each row lists the change required
and the P16 phase that owns it. None are silent races today — the
current single-threaded runtime is the documented contract.

| # | location | nature | P16 retrofit | owner phase |
|---|---|---|---|---|
| A.1 | `torajs-mmalloc/src/core.rs:222` | `static mut CORE_ALLOC: Allocator` + `CORE_LOCK: AtomicBool` guard | already cross-thread-safe via AtomicBool spinlock; multi-thread perf wants finer-grained per-size-class lock | P16.2 |
| A.2 | `torajs-mmalloc/src/core.rs:223` | `static mut CORE_REGISTRY: SpanRegistry` (same lock) | same as A.1 | P16.2 |
| A.3 | `torajs-mmalloc/src/core.rs:244` | `pub static mut __torajs_core_tlab: TlabCache` — **process-wide TLAB** | **per-thread TLAB** via syscall-thread-id-indexed manual array. **NOT `#[thread_local]`** (Darwin local-exec TLS routes via `__tlv_bootstrap` libSystem; conflicts with 0-libc — see "0-libc tension" below). Source comment L233-239 already documents this v0.8 plan. | P16.2 |
| A.4 | `torajs-meta/src/classmeta.rs:53-54` | `static mut PROTOS_BY_TAG_IMM` / `CLASSES_BY_TAG_IMM` | class meta tables — init-once then read-only. Convert to `OnceLock<[u64; MAX_CLASSES]>` (atomic publish at init; lock-free read after) | P16.4 (Send/Sync semantic) |
| A.5 | `torajs-num/src/math.rs:236` | `static mut RNG_STATE: (u64, u64)` | per-thread RNG via thread-id-indexed array (JVM/Node/Go all per-thread) | P16.x |
| A.6 | `torajs-microtask/src/lib.rs:208` | `static mut COUNTER: i64` inside `next_microtask_id` | per-thread counter (microtask IDs are per-isolate) | P16.5 (Worker API) |
| A.7 | `torajs-microtask/src/lib.rs:230` | `static mut REENQUEUE_BUDGET: i32` | per-thread budget — microtask queue is per-thread by spec | P16.5 |
| A.8 | `torajs-date/src/tz.rs:174-175` | `static mut TZ_CACHE: Option<Tz>` / `TZ_TRIED: bool` | `OnceLock<Tz>` — TZ probe is once-per-process | P16 cleanup pass |
| A.9 | `torajs-throw/src/lib.rs:162,168,174` | `AtomicI64 THROW_ACTIVE / THROW_TAG / THROW_VALUE` (global, already atomic) | exception is per-thread state by spec — convert to thread-id-indexed array (atomic is wasted today since single-mutator; will become correctness-required in multi-thread) | P16.4 |

Eight of nine fall into two P16 phases (.2 allocator, .5 Worker API,
.4 Send/Sync). The atomic-vs-thread-local choice on A.3/A.5/A.6/A.7/A.9
is constrained by the 0-libc tension below.

## Group B — production locked state (multi-thread-compatible today)

These are already `Mutex`-protected and multi-thread-correct. Listed
for completeness; the question for P16 is "is the lock granularity OK
under N threads" — not "does it work at all".

| location | content | multi-thread assessment |
|---|---|---|
| `torajs-str/src/symbol.rs:223` | `static SYMBOL_REG: Mutex<Vec<usize>>` | low-frequency writes (`Symbol.for` only), Mutex acceptable for v1 multi-thread |
| `torajs-promise/src/unhandled.rs:97` | `static UNHANDLED_LIST: Mutex<Vec<i64>>` | per-rejection writes, low frequency, OK |
| `torajs-meta/src/fnprops.rs:71` | `static FNPROPS_LOCK: Mutex<()>` (note: replaces v0.5 `OnceLock<Mutex<HashMap<usize, usize>>>`) | already redesigned post-v0.5 for low contention |
| `torajs-process/src/lib.rs:207,212` | `ARGV_STATE` / `ENVP_STATE: Mutex<...>` | CLI bootstrap, init-once, no contention |

## Group C — compile-time / test-only (out of multi-thread scope)

- `crates/torajs-core/src/ssa_inkwell.rs:126` — `pub(super) static COMPILE_LOCK: Mutex<()>`. Host-side compile lock (LLVM inkwell is not thread-safe). Not in the user-binary runtime path. **No P16 action.**
- Multiple `static TEST_LOCK: Mutex<()>` inside `#[cfg(test)] mod tests {...}` across `torajs-str` / `torajs-microtask` / `torajs-throw`. **No P16 action.**

---

## 0-libc tension — TLS in user binaries

**The single highest-impact framing finding.**

`torajs-mmalloc/src/core.rs:224-239` source comment (verbatim):

> Step 16-c-2 (2026-05-29): downgraded from `#[thread_local]` to a
> plain `static mut` to drop the last `__tlv_bootstrap` undefined
> symbol from user binaries (A5 zero-libc-undef goal). On macOS
> aarch64 `#[thread_local]` forces a `$tlv$init` / `__tlv_bootstrap`
> dyld dependency — see docs/v0.7-A5-finding.md. The single-threaded
> runtime has no concurrent observer, so a process-wide TLAB is sound.
>
> MULTI-THREAD RE-DERIVATION (v0.8 backlog): a process-wide TLAB
> defeats the per-thread isolation a threaded runtime needs. When the
> first threaded path lands (Promise/async/worker), re-derive per-
> thread TLABs via a syscall-thread-id-indexed manual array (NOT
> `#[thread_local]` — Darwin local-exec TLS still routes via tlv).

**Implication**: vision axis A5 (0-libc) and vision axis D
(multi-thread-ready substrate) collide on Darwin aarch64 TLS. Resolution
is fixed: **use a syscall-thread-id-indexed manual array** (we have a
syscall trampoline already; the array is just `[Slot; MAX_THREADS]`
indexed by `pthread_self() & MASK` equivalent). This satisfies both
constraints — 0 `__tlv_bootstrap` dependency *and* per-thread isolation.

The Group A retrofits A.3 / A.5 / A.6 / A.7 / A.9 all share this
mechanism. **Designing it once in P16.2 unlocks the rest of Group A
for one-line storage substitutions.**

---

## Partial framing-readiness already shipped

The mmalloc author already designed the cross-thread free queue for
P16 use:

`torajs-mmalloc/src/core.rs` `static CORE_CENTRAL: CentralQueue` —
verbatim source comment:

> Process-wide central queue (Phase 2c item 10). Lock-free MPMC
> stack per size class; acts as the TLAB overflow buffer + cross-
> thread free landing zone. […] Multi-thread future: foreign-thread
> free → Central.push automatically routes to owning thread's next
> refill cycle.

This means P16.2 (shared heap allocator) is an **augment-not-rewrite**:
- per-thread TLAB plumbing (Group A.3) plus
- routing `foreign_thread_free → CORE_CENTRAL.push` (already exists, just unwired)

Estimated ROI: P16.2 is much smaller than the typical "implement a
multi-threaded allocator from scratch" task.

---

## Substrate compatibility verdict

Audit pass result: **no immediate retrofit commits required**.

- Current chunk 9d splice and P2 RC emit hook do not introduce new
  single-mutator hot paths
- All existing single-mutator sites carry either an `AtomicBool`
  spinlock guard (mmalloc) or `AtomicI64` (throw) or documented
  intentional process-wide read-only data (classmeta)
- §6.2 HARD RULE detection grep (`self.intrinsics.rc_inc\b`) is
  clean post-P2: 0 direct emits anywhere outside the helper

P16+ design phase starts from this audit. New ssa-lower / runtime work
between now and v1.0 must not introduce new Group A sites; if it does,
the `lazy_static<Mutex<...>>` / `static mut` grep must call them out
in pre-flight and they go straight to L3b with a Group A row.

---

## Action items (consumed by plan-state L3b)

- L3b watch item: **multi-thread-ready substrate audit baseline locked at this audit**. New `static mut` / `lazy_static<Mutex>` / `OnceLock<Mutex>` / `#[thread_local]` outside this audit's Group A/B/C tables = §6.2 violation, must be flagged at pre-flight.
- L3b watch item: **0-libc vs multi-thread TLS tension** — Group A retrofits A.3/A.5/A.6/A.7/A.9 all share the syscall-thread-id-indexed array mechanism; design once in P16.2.
- P16.2 backlog: **mmalloc partial framing already shipped** — `CORE_CENTRAL` cross-thread free queue exists, P16.2 work is plumbing per-thread TLAB and routing foreign-thread free into it.

---

## Audit reproduction

Run from repo root:

```bash
grep -rnE '^[[:space:]]*static mut\b|pub static mut\b' --include='*.rs' crates/
grep -rnE 'static.*Mutex<|static.*RwLock<'             --include='*.rs' crates/
grep -rnE 'OnceLock<Mutex|OnceLock<RwLock'             --include='*.rs' crates/
grep -rnE 'lazy_static!|once_cell::sync::Lazy.*Mutex'  --include='*.rs' crates/
grep -rnE '__thread\b|#\[thread_local\]|thread_local!' --include='*.rs' crates/
```

Each hit must map to exactly one row in this audit (A.x / B.x / C.x).
New hits without a row = §6.2 violation at pre-flight.
