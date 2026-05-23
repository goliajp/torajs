/*
 * torajs C runtime — non-Copy heap object layout + string / array helpers.
 *
 * ===== Universal heap object header (Phase B refcount) =====
 *
 * Every non-Copy heap allocation (Str today; Obj / Arr / Closure in
 * Phase 2) begins with this 8-byte header. The runtime uses it for
 * reference counting (B-style ARC: inc on share, dec on drop, free at 0)
 * and reserves bits for future GC / weak-ref / debug extensions.
 *
 *   offset 0: refcount (u32) — initial 1; never zero except just-before-free
 *   offset 4: type_tag (u16) — 0=str, 1=obj, 2=arr, 3=closure
 *   offset 6: flags    (u16) — reserved (weak / mark / cycle bits)
 *
 * Type-specific metadata follows immediately at offset 8.
 *
 *   Str:     [header:8][len:8][bytes:N]              prefix 16
 *   Arr:     [header:8][len:8][cap:8][slots:N*8]     prefix 24 (Phase 2)
 *   Obj:     [header:8][type_id:4][vtable:8][...]    Phase 2
 *   Closure: [header:8][fn_addr:8][drop_fn:8][...]   Phase 2
 *
 * Phase 1 (this file + ssa_inkwell.rs str defs): Str migrated.
 * Phase 2 (later commits): Arr / Obj / Closure migrated.
 */

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* T-20 (v0.6.0) — wasi-libc has neither execinfo.h (no native frame
 * walk on wasm) nor mach-o/dyld.h (no dyld). Gate both so the C
 * runtime compiles for wasm32-wasip1; the panic helper degrades
 * gracefully to "msg + exit(1)" without backtrace. */
#ifndef __wasi__
#include <execinfo.h>
#endif

#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif

/* v0.3 #4 D-4 — central panic helper used by every C-side runtime
 * fatal-error path. Prints the message + a libc-backtrace of raw PC
 * addresses; on macOS, shells out to `atos` per frame to resolve to
 * `<binary>:<line>:<col>` using the .dSYM bundle next to the binary
 * (created by `dsymutil` in the link pipeline). On linux, prints raw
 * PCs that the user can resolve with `addr2line -e binary <pc>`.
 *
 * Marked noreturn so callers don't need to `exit(1)` after — keeps
 * fputs+exit pairs from drifting out of sync at refactor time. */
static char self_path_buf_[4096];
static const char *torajs_self_path_(void) {
    if (self_path_buf_[0] != '\0') return self_path_buf_;
#ifdef __APPLE__
    uint32_t sz = sizeof(self_path_buf_);
    if (_NSGetExecutablePath(self_path_buf_, &sz) == 0) {
        return self_path_buf_;
    }
#else
    ssize_t n = readlink("/proc/self/exe", self_path_buf_, sizeof(self_path_buf_) - 1);
    if (n > 0) {
        self_path_buf_[n] = '\0';
        return self_path_buf_;
    }
#endif
    self_path_buf_[0] = '?';
    self_path_buf_[1] = '\0';
    return self_path_buf_;
}

__attribute__((noreturn))
void __torajs_panic(const char *msg) {
    fputs(msg, stderr);
    fputc('\n', stderr);
#ifdef __wasi__
    /* T-20 — wasm32-wasip1 has no backtrace facility; just exit. */
    exit(1);
#else
    /* "not yet supported:" prefix is the test262 / conformance
     * runner's signal that this is an intentional substrate-
     * boundary rejection, not a true crash. Emitting a backtrace
     * here would shift the case from `incompatible` to `bug` in
     * the test262 classifier. Skip backtrace for these. */
    int suppress_bt = (strncmp(msg, "not yet supported:", 18) == 0);
    void *frames[32];
    int n = suppress_bt ? 0 : backtrace(frames, 32);
    if (n > 1) {
        const char *path = torajs_self_path_();
        fputs("backtrace:\n", stderr);
#ifdef __APPLE__
        /* atos -o <binary> -arch arm64 -l <slide> <pc1> <pc2> ... —
         * macOS ASLR slides the image at load time; atos needs the
         * slide via `-l` to translate runtime PCs back to static
         * addresses in the binary's __TEXT. _dyld_get_image_vmaddr_slide(0)
         * returns the slide of the main executable. One fork+exec per
         * panic, prints `fn (in binary) (file:line)` per line. */
        /* atos resolves cleanly when given STATIC addresses (PC -
         * runtime_slide). The `-l <slide>` flag in atos seems to
         * misbehave on recent macOS for arm64 dSYM-based input —
         * subtracting slide ourselves works reliably. */
        intptr_t slide = _dyld_get_image_vmaddr_slide(0);
        char cmd[8192];
        int off = snprintf(
            cmd, sizeof(cmd),
            "atos -o '%s' -arch arm64",
            path
        );
        for (int i = 1; i < n && off < (int)sizeof(cmd) - 32; i++) {
            uintptr_t static_pc = (uintptr_t)frames[i] - (uintptr_t)slide;
            off += snprintf(cmd + off, sizeof(cmd) - off, " 0x%lx", (unsigned long)static_pc);
        }
        snprintf(cmd + off, sizeof(cmd) - off, " 1>&2");
        int _ = system(cmd);
        (void)_;
#else
        /* linux: raw PCs; user can `addr2line -e binary <pc>` */
        for (int i = 1; i < n; i++) {
            fprintf(stderr, "  %p (in %s)\n", frames[i], path);
        }
#endif
    }
    exit(1);
#endif /* !__wasi__ */
}


/* defined by the inkwell-emitted LLVM IR in the AOT binary */
void *__torajs_arr_alloc(uint64_t initial_cap);
void *__torajs_arr_push(void *arr, int64_t val);

/* ============================================================
 * Universal heap-object header + refcount API
 * ============================================================ */

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_STR     0
#define __TORAJS_TAG_OBJ     1
#define __TORAJS_TAG_ARR     2
#define __TORAJS_TAG_CLOSURE 3
#define __TORAJS_TAG_REGEX   4  /* runtime_regex.c — compiled NFA + flags */
#define __TORAJS_TAG_DATE    5  /* runtime_date.c — { ms_since_epoch } */
#define __TORAJS_TAG_ANY_BOX 6  /* T-10.d — boxed Type::Any: header + tag + value */
#define __TORAJS_TAG_SYMBOL  7  /* T-13.a — Symbol value: header + desc str ptr */
/* TAG 8 = Promise (runtime_promise.c) — its drop hook handles its own
 * universal-header free dispatch, not routed through value_drop_heap. */
#define __TORAJS_TAG_RESPONSE 9 /* T-21 — fetch() Response: header + status + body Str* */
#define __TORAJS_TAG_BIGINT  10 /* T-25 — BigInt: header + sign u32 + len u32 + words[len] (little-endian u64 magnitude) */
#define __TORAJS_TAG_WEAKREF 11 /* T-26 — WeakRef: header + target ptr (NULL = target reclaimed) */
#define __TORAJS_TAG_WEAKMAP 12 /* T-26.B — WeakMap: header + bucket table; entries observed via weakref registry */
#define __TORAJS_TAG_WEAKSET 13 /* T-26.B — WeakSet: same as WeakMap minus the value side */
#define __TORAJS_TAG_MAP     15 /* P6.1 — strong-ref Map<K,V>: header + n_entries + capacity + tombstones + entries ptr */
#define __TORAJS_TAG_MAP_ITER 16 /* P6.4b — stateful Map iterator: header + map_ptr + cursor + kind */
#define __TORAJS_TAG_ARR_ITER 17 /* P6.4c-C3 — stateful Array<Any> iterator (parallel to MapIter) */
#define __TORAJS_TAG_DYNOBJ  14 /* P3.1 — dynamic-property object (HashMap-backed):
                                 * header(8) + count(u32) + cap(u32) +
                                 * tombstones(u32) + pad(u32) + buckets[cap] of
                                 * { key_ptr: *Str, tag: u64, value: u64 } (24 bytes).
                                 * Open addressing, linear probing. Tombstone =
                                 * key_ptr == 0x1 (sentinel). Used for Type::Any
                                 * untyped object slot; typed Type::Obj keeps the
                                 * static-layout struct (tag=OBJ) for hot-path perf. */

/* T-26.C — Bacon-Rajan cycle collector colors. 2 bits at flag bit 3-4
 * encode the trial-deletion state of each non-Copy heap object:
 *
 *   BLACK  (0) — in use, no cycle suspicion
 *   GRAY   (1) — being marked during a current trial-deletion pass
 *   PURPLE (2) — buffered as a potential cycle root (rc went down but
 *                stayed > 0 on a cyclic-shape type)
 *   WHITE  (3) — confirmed garbage; freed by collect phase
 *
 * Bit 5 = BUFFERED — fast "is this in the cycle buffer right now"
 * gate so every rc_dec doesn't traverse the buffer to dedup. Bits
 * 6-15 stay free for future substrate. */
#define __TORAJS_COLOR_SHIFT  3u
#define __TORAJS_COLOR_MASK   (3u << __TORAJS_COLOR_SHIFT)
#define __TORAJS_COLOR_BLACK  (0u << __TORAJS_COLOR_SHIFT)
#define __TORAJS_COLOR_GRAY   (1u << __TORAJS_COLOR_SHIFT)
#define __TORAJS_COLOR_PURPLE (2u << __TORAJS_COLOR_SHIFT)
#define __TORAJS_COLOR_WHITE  (3u << __TORAJS_COLOR_SHIFT)
#define __TORAJS_FLAG_BUFFERED (1u << 5)

/* T-10.b (v0.4.0) — Type::Any tagged-slot tags. An Array<Any> stores
 * 16-byte slots `{ tag: u64 (low 8 bits used), value: u64 }` so each
 * slot self-describes its contents. ANY_NULL / ANY_BOOL / ANY_I64 /
 * ANY_F64 stash the value inline; ANY_HEAP stashes a pointer to a
 * heap object whose actual type is discoverable via the universal
 * heap header's `type_tag` field (Str / Obj / Arr / Closure / RegExp
 * / Date / nested Any-array). T-10.c wires the codegen path that
 * emits these slots from heterogeneous Array literals. */
#define __TORAJS_ANY_NULL    0
#define __TORAJS_ANY_BOOL    1
#define __TORAJS_ANY_I64     2
#define __TORAJS_ANY_F64     3
#define __TORAJS_ANY_HEAP    4
/* P1.2 — distinct tag for the `undefined` value, separate from
 * ANY_NULL=0. Per ES spec §6.1.1 / §6.1.2 null and undefined are
 * different primitive values (`typeof null === "object"` vs
 * `typeof undefined === "undefined"`, `null !== undefined`). Pre-
 * P1.2 tora collapsed both to ANY_NULL which silently wrong-ed
 * the typeof distinction and made all undefined-vs-null comparisons
 * identity-positive. ANY_UNDEF=5 lets the box helpers preserve
 * the distinction; the per-op rules (typeof, strict-eq, to-bool,
 * etc.) still mirror Null where the spec says they should
 * (e.g. ToBoolean(undefined) is false — same as null). */
#define __TORAJS_ANY_UNDEF   5

/* Array<Any> uses 16-byte slots instead of 8. Marked at alloc time
 * via this flag bit so arr_drop_any can walk slots correctly and
 * arr_free can route Any-arrays out of the regular arr_pool (the
 * pool is sized for 8-byte-slot caps; mixing strides corrupts it). */
#define __TORAJS_FLAG_ARR_ANY 8u

/* T-09.d (v0.4.0) — `Object.freeze(obj)` sets this bit. Field write
 * codegen emits a runtime check: if set, skip the store (silent
 * ignore matches JS spec non-strict mode, which tr defaults to —
 * tr has no `"use strict"` directive). isFrozen reads this bit
 * directly. */
#define __TORAJS_FLAG_FROZEN 16u

/* Universal heap header flag bits.
 *   bit 1 (=2): SPLIT_BLOCK — single-malloc block produced by str_split,
 *               carries N inline substr structs; routed to split_pool on free.
 *   bit 2 (=4): STATIC_LITERAL — block lives in the LLVM module's .rodata
 *               (emitted by ssa_inkwell as a static Str-shaped global).
 *               rc_inc / rc_dec / str_free / arr_free no-op so the same
 *               global serves every callsite of a literal across a hot
 *               loop without per-iter alloc + memcpy + drop. Initial
 *               refcount value is irrelevant since rc_inc/dec skip it.
 */
#define __TORAJS_FLAG_STATIC_LITERAL 4u

/* P2.2 (2026-05-22 architecture-rewrite) — `__torajs_rc_inc` and
 * `__torajs_rc_dec` are now provided by the Rust `torajs-rc` crate
 * (Layer 1 of the layered architecture; see
 * docs/architecture-rewrite.md). Their C definitions used to live
 * here; deleted at P2.2 ship along with the universal-heap-header
 * refcount intrinsics, per vision item #3 (pure rust).
 *
 * ABI is unchanged — same symbol names, same calling convention,
 * same byte-for-byte semantics (NULL pass-through,
 * FLAG_STATIC_LITERAL bypass, WeakRef-on-zero hook ordering). The
 * `extern` declarations below let the rest of runtime_str.c
 * (and the other runtime_*.c files via their own externs) keep
 * calling them; the linker resolves them from libtorajs_rc.a at
 * `tr build` time.
 *
 * `__torajs_weakref_target_dying` stays defined in
 * runtime_weakref.c; torajs-rc declares it `extern` and calls it
 * on rc-hit-zero just as the old C version did. */
extern void __torajs_rc_inc(void *p);
extern int __torajs_rc_dec(void *p);
extern void __torajs_weakref_target_dying(void *target);

/* ============================================================
 * Str layout helpers
 *
 * Str = [header:8][len:8][bytes:N]
 * len  at offset 8
 * data at offset 16
 * ============================================================ */

#define __TORAJS_STR_HDR_SIZE   16
#define __TORAJS_STR_LEN(p)     (*(uint64_t *)((const uint8_t *)(p) + 8))
#define __TORAJS_STR_DATA(p)    ((uint8_t *)(p) + __TORAJS_STR_HDR_SIZE)
#define __TORAJS_STR_CDATA(p)   ((const uint8_t *)(p) + __TORAJS_STR_HDR_SIZE)

/* Internal helper — alloc a fresh Str heap with refcount=1 + type_tag set
 * + len written. Caller fills the bytes payload at __TORAJS_STR_DATA(p).
 * Single-source-of-truth for every Str allocation in this file.
 *
 * Header is one combined u64 store (refcount=1 in low 32 bits +
 * type_tag=STR in [32:48] + flags=0 in [48:64]) instead of three
 * separate stores. Cuts the per-alloc store count to 2 (header + len). */
#define __TORAJS_STR_HEADER_INIT \
    ((uint64_t)1u | ((uint64_t)__TORAJS_TAG_STR << 32))

/* ============================================================
 * Small-Str pool + Str alloc/free moved to the `torajs-str` Rust
 * sub-crate (P3.1-a, 2026-05-23). The two cross-TU symbols
 * `__torajs_str_alloc_pooled` and `__torajs_str_free` are now
 * defined by `libtorajs_str.a`; this file (and other runtime_*.c
 * TUs) call them via the forward decls below.
 *
 * Layout / packing constants kept identical:
 *   header + 16-byte payload = 32-byte pooled block, 32 LIFO slots.
 *   __TORAJS_STR_HEADER_INIT = 1 | (TAG_STR << 32).
 *   __TORAJS_STR_HDR_SIZE = 16, __TORAJS_STR_LEN(p) at offset 8.
 *
 * Rationale: pillar 3 (pure rust) — the alloc/free hot path moves
 * into a self-contained sub-crate so the substrate stays pure-rust
 * end-to-end as later sub-steps (P3.1-c eq/concat/to_number /
 * P3.1-d lookup / P3.1-e transform / P3.1-f split) port their
 * callers off `__torajs_str_alloc_pooled` and onto direct Rust API.
 * ============================================================ */

extern uint8_t *__torajs_str_alloc_pooled(uint64_t len);
extern void __torajs_str_free(uint8_t *p);

/* ============================================================
 * Substr layout (Phase Substr.A — substring view)
 *
 * Substr = [header:8][len:8][parent_ptr:8][offset:8]   total 32
 *   parent_ptr → owned Str whose bytes the view references
 *   offset     → byte offset into parent.bytes where view starts
 *   data       = parent.bytes + offset (computed on access; not stored)
 *
 * Refcount semantics: substr's drop dec's its own refcount; when 0 it
 * also dec's parent's refcount (via str_drop) before freeing self.
 * Parent stays alive as long as ANY view into it exists.
 *
 * Why a separate type from Str: keeps OWNED Str layout (16-byte
 * prefix, bytes inline at +16) untouched — hot-path byte access on
 * an owned Str stays a single GEP, no indirection. View access pays
 * one extra load (parent_ptr → +16 → bytes), but only on Substr-typed
 * values. Mirrors Swift's `String` / `Substring` split (or Rust's
 * `String` / `&str`).
 * ============================================================ */

#define __TORAJS_SUBSTR_SIZE        32
#define __TORAJS_SUBSTR_PARENT_OFF  16
#define __TORAJS_SUBSTR_OFFSET_OFF  24
#define __TORAJS_SUBSTR_LEN(p)      (*(uint64_t *)((const uint8_t *)(p) + 8))
#define __TORAJS_SUBSTR_PARENT(p)   (*(uint8_t **)((const uint8_t *)(p) + __TORAJS_SUBSTR_PARENT_OFF))
#define __TORAJS_SUBSTR_OFFSET(p)   (*(uint64_t *)((const uint8_t *)(p) + __TORAJS_SUBSTR_OFFSET_OFF))

/* Substr flag bits (in the universal-header `flags` field).
 *
 * INLINE — substr struct is embedded inside another allocation (e.g.
 * the array tail emitted by __torajs_str_split's single-block layout).
 * substr_drop must NOT free the substr itself in that case; the
 * enclosing block's drop frees everything in one go. The drop still
 * dec's the parent Str's refcount because each inline substr holds
 * one parent ref. */
#define __TORAJS_FLAG_SUBSTR_INLINE 1u

/* Forward-declare: __torajs_str_drop is defined in inkwell IR (see
 * `define_str_drop` in ssa_inkwell.rs). We call it on parent at view-
 * drop time; the linker resolves the symbol. */
void __torajs_str_drop(void *s);

/* Substr cell pool + __torajs_substr_create / __torajs_substr_drop
 * moved to the `torajs-str::substr` Rust module (P3.1-b, 2026-05-23).
 * The two cross-TU symbols are now defined by `libtorajs_str.a`;
 * remaining str fns in this file (and other runtime_*.c) call them
 * via the forward decls below. The SplitBlock pool (FLAG_SPLIT_BLOCK
 * + split_pool_blocks_) remains C-resident for now because it shares
 * dispatch with `__torajs_arr_free` (Layer-3 territory, not yet
 * ported). P3.1-f / P4 ports that. */
extern void *__torajs_substr_create(void *parent, uint64_t offset, uint64_t len);
extern void __torajs_substr_drop(void *v);

/* str_split-block pool — keeps the variable-size single-block
 * allocations made by __torajs_str_split (header + N ptr slots +
 * N inline 32-byte substr structs) in a small thread-local cache
 * indexed by `cap`. Tight loops over `s.split(sep)` recycle the
 * exact same block every iter, turning each split's malloc into
 * a pointer-pop. The block carries SPLIT_BLOCK in its flags so
 * arr_drop knows which pool to push to.
 *
 * Bounded slot count + per-cap match keeps the search O(N)-with-
 * tiny-N (rare to see > a handful of distinct cap values in a
 * single tight loop). */
#define __TORAJS_FLAG_SPLIT_BLOCK 2u
#define __TORAJS_SPLIT_POOL_SLOTS 16
static uint8_t *split_pool_blocks_[__TORAJS_SPLIT_POOL_SLOTS];
static uint64_t split_pool_caps_[__TORAJS_SPLIT_POOL_SLOTS];
static int split_pool_count_ = 0;

/* Generic Array LIFO pool — keyed by cap, bounded slot count.
 *
 * Hot in any code path that produces fresh small arrays inside a
 * tight loop (literal `[a, b, c]` allocations in fn-local scopes,
 * working-set scratch arrays, etc.). Earlier benchmarks (rpn-eval-
 * 100k, 16-elem stack literal allocated per call) showed every
 * iter paying a malloc + 24-byte header init; pool turns that into
 * a pointer-pop + cap-match scan.
 *
 * Cap-indexed (not size-class-rounded) so `[1,2,3]` (cap 3) and
 * `[1,2,3,4]` (cap 4) don't share a slot — keeps every block right-
 * sized for its caller. The cap match is O(arr_pool_count_) but
 * tight loops typically see one dominant cap value, so the LIFO
 * head matches on the first compare.
 *
 * Cap > __TORAJS_ARR_POOL_CAP_MAX bypasses the pool and pays a
 * direct malloc/free — the leverage is concentrated in small literal
 * allocs, large arrays' alloc cost is amortized across their use. */
#define __TORAJS_ARR_POOL_SLOTS    16
#define __TORAJS_ARR_POOL_CAP_MAX  32
static uint8_t *arr_pool_blocks_[__TORAJS_ARR_POOL_SLOTS];
static uint64_t arr_pool_caps_[__TORAJS_ARR_POOL_SLOTS];
static int arr_pool_count_ = 0;

/* Hardcoded constants — Arr layout macros are declared further down
 * in this file but the split pool needs them now. Keep in sync with
 * `__TORAJS_ARR_HDR_SIZE` (24) + `__TORAJS_ARR_CAP_OFF` (16). */
static inline uint8_t *split_block_alloc_(uint64_t out_count) {
    for (int i = 0; i < split_pool_count_; i++) {
        if (split_pool_caps_[i] == out_count) {
            uint8_t *p = split_pool_blocks_[i];
            int last = --split_pool_count_;
            split_pool_blocks_[i] = split_pool_blocks_[last];
            split_pool_caps_[i] = split_pool_caps_[last];
            return p;
        }
    }
    uint64_t slots_size = out_count * 8;
    uint64_t substrs_size = out_count * __TORAJS_SUBSTR_SIZE;
    uint64_t total = 24 + slots_size + substrs_size;
    return (uint8_t *)malloc(total);
}

/* Free path for Array blocks. arr_drop (inkwell IR) calls
 * __torajs_arr_free at refcount=0. Dispatch order:
 *   - SPLIT_BLOCK flag set → split_pool (split.c built block,
 *     contains inline substr structs; size class includes the
 *     substr area)
 *   - cap ≤ POOL_CAP_MAX, pool not full → arr_pool (small
 *     fn-local literal arrays; recycled by next equal-cap alloc)
 *   - otherwise → libc free
 *
 * Hardcoded `(uint8_t *)p + 16` reads cap (ARR_HDR_CAP_OFF). */
void __torajs_arr_free(void *p) {
    if (p == NULL) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    /* Defense-in-depth: keep in sync with str_free / rc_inc / rc_dec. */
    if (h->flags & __TORAJS_FLAG_STATIC_LITERAL) return;
    /* T-13.5 — cap shrunk to u32; high 32 bits hold head_offset. Only
     * read the low 32 here. (For pooled arrays, the head value can
     * be reset to 0 on next alloc so we don't need to preserve it.) */
    uint64_t cap = (uint64_t)(*(uint32_t *)((uint8_t *)p + 16));
    if (h->flags & __TORAJS_FLAG_SPLIT_BLOCK) {
        if (split_pool_count_ < __TORAJS_SPLIT_POOL_SLOTS) {
            split_pool_blocks_[split_pool_count_] = (uint8_t *)p;
            split_pool_caps_[split_pool_count_] = cap;
            split_pool_count_++;
            return;
        }
    } else if (cap <= __TORAJS_ARR_POOL_CAP_MAX
               && arr_pool_count_ < __TORAJS_ARR_POOL_SLOTS
               && !(h->flags & __TORAJS_FLAG_ARR_ANY)) {
        /* Array<Any> uses 16-byte slots; the pool is sized for the
         * regular 8-byte stride so mixing the two corrupts subsequent
         * pulls. Route Any-arrays straight to libc free.
         */
        arr_pool_blocks_[arr_pool_count_] = (uint8_t *)p;
        arr_pool_caps_[arr_pool_count_] = cap;
        arr_pool_count_++;
        return;
    }
    free(p);
}

/* Pool-aware Array alloc. Counterpart to inkwell's old
 * `__torajs_arr_alloc` body — the IR side now collapses to a
 * single tail call here.
 *
 * cap ≤ POOL_CAP_MAX → scan arr_pool LIFO-end-first for a matching
 * cap; hit pops the slot in O(1) (the LIFO head is what tight
 * loops produce/consume). Miss falls through to malloc.
 *
 * Header init: rc=1 / tag=ARR / flags=0 / len=0 / cap as passed.
 * Same field layout the inkwell-emitted version used. */
void *__torajs_arr_alloc_pooled(uint64_t cap) {
    uint8_t *p = NULL;
    if (cap <= __TORAJS_ARR_POOL_CAP_MAX && arr_pool_count_ > 0) {
        for (int i = arr_pool_count_ - 1; i >= 0; i--) {
            if (arr_pool_caps_[i] == cap) {
                p = arr_pool_blocks_[i];
                int last = --arr_pool_count_;
                arr_pool_blocks_[i] = arr_pool_blocks_[last];
                arr_pool_caps_[i] = arr_pool_caps_[last];
                break;
            }
        }
    }
    if (p == NULL) {
        /* 24 = __TORAJS_ARR_HDR_SIZE (defined further down; same
         * forward-decl trick as split_block_alloc_ uses). */
        p = (uint8_t *)malloc(24 + (size_t)cap * 8);
    }
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_ARR;
    h->flags = 0;
    *(uint64_t *)(p + 8) = 0;          /* len */
    *(uint32_t *)(p + 16) = (uint32_t)cap;  /* cap (u32) */
    *(uint32_t *)(p + 20) = 0;         /* T-13.5 — head_offset (u32) */
    return p;
}

/* ============================================================
 * T-10.b (v0.4.0) — Array<Any> tagged-slot runtime
 *
 * Layout: same 24-byte header (refcount/type_tag/flags + len + cap)
 * but slot stride is 16 bytes (vs 8 for regular Array<T>):
 *
 *     [hdr 24][slot0 16][slot1 16] ...
 *      where slot = { tag: u64 (low 8 bits used), value: u64 }
 *
 * `flags` carries `__TORAJS_FLAG_ARR_ANY` so arr_free routes the
 * block out of the regular arr_pool (whose slot-stride assumption
 * doesn't match) and arr_drop_any (vs arr_drop) is the correct
 * walker. The header's `type_tag` stays `TAG_ARR` so generic ARC
 * intrinsics (rc_inc / rc_dec / heap walk) treat it like any other
 * array; the dispatch on Any-vs-non-Any happens at the codegen
 * call site, which already knows whether it's emitting Array<Any>
 * or Array<T>.
 *
 * T-10.b ships only the runtime helpers — codegen wiring lands
 * with T-10.c. The helpers are dead code until T-10.c calls into
 * them; included now so the C side can compile + the symbols are
 * ready for the inkwell decls.
 * ============================================================ */

/* Slot stride for Array<Any>. */
#define __TORAJS_ANY_SLOT_BYTES  16

/* slot[i] tag pointer (writable). */
static inline uint64_t *any_slot_tag_(void *arr, uint64_t i) {
    return (uint64_t *)((uint8_t *)arr + 24 /* __TORAJS_ARR_HDR_SIZE */
                        + i * __TORAJS_ANY_SLOT_BYTES);
}

/* slot[i] value pointer (writable). */
static inline uint64_t *any_slot_val_(void *arr, uint64_t i) {
    return (uint64_t *)((uint8_t *)arr + 24 /* __TORAJS_ARR_HDR_SIZE */
                        + i * __TORAJS_ANY_SLOT_BYTES + 8);
}

void *__torajs_arr_alloc_any(uint64_t cap) {
    /* Allocate header + cap × 16-byte slots. Bypass the pool
     * (different stride). */
    uint8_t *p = (uint8_t *)malloc(24 /* __TORAJS_ARR_HDR_SIZE */
                                   + (size_t)cap * __TORAJS_ANY_SLOT_BYTES);
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_ARR;
    h->flags = __TORAJS_FLAG_ARR_ANY;
    *(uint64_t *)(p + 8) = 0;          /* len */
    *(uint32_t *)(p + 16) = (uint32_t)cap;  /* cap (u32) */
    *(uint32_t *)(p + 20) = 0;         /* T-13.5 — head_offset (Array<Any>
                                        * never shifts; stays 0). */
    return p;
}

/* P0.10 — `new Array(n)` numeric form per ES spec §23.1.2.1.
 * Allocates an Array<Any> of length n with all slots set to
 * ANY_NULL (tag=0, value=0). Behaves like a sparse array but
 * with explicit null-fill so arr[i] reads return null
 * (matches JS's `undefined` in the typed-as-Any context). */
void *__torajs_arr_alloc_any_filled(uint64_t n) {
    uint8_t *p = (uint8_t *)malloc(24 + (size_t)n * __TORAJS_ANY_SLOT_BYTES);
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_ARR;
    h->flags = __TORAJS_FLAG_ARR_ANY;
    *(uint64_t *)(p + 8) = n;                /* len = n */
    *(uint32_t *)(p + 16) = (uint32_t)n;     /* cap = n */
    *(uint32_t *)(p + 20) = 0;
    /* Zero the slots — both tag (0=ANY_NULL) and value (0). */
    if (n > 0) {
        memset(p + 24, 0, (size_t)n * __TORAJS_ANY_SLOT_BYTES);
    }
    return p;
}

/* Append a tagged slot. Grows by 2× when len == cap (matches
 * `__torajs_arr_push`'s growth strategy). Returns the (possibly
 * realloc'd) array pointer; caller stores it back into the slot,
 * mirroring the `arr_push` contract.
 *
 * For ANY_HEAP slots, the caller is responsible for having
 * incremented the heap value's refcount BEFORE calling — push
 * takes ownership of the pre-bumped reference and will dec it via
 * `arr_drop_any` when the array dies. */
void *__torajs_arr_push_any(void *arr, uint64_t tag, uint64_t value) {
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)arr;
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    uint32_t cap = *(uint32_t *)((uint8_t *)arr + 16);
    if ((uint32_t)len == cap) {
        uint32_t new_cap = cap == 0 ? 4 : cap * 2;
        uint8_t *grown = (uint8_t *)realloc(
            arr,
            24 /* __TORAJS_ARR_HDR_SIZE */ + (size_t)new_cap * __TORAJS_ANY_SLOT_BYTES);
        arr = grown;
        h = (__torajs_heap_header_t *)arr;
        *(uint32_t *)((uint8_t *)arr + 16) = new_cap;
        /* head_offset stays 0 for Any-arrays. */
    }
    *any_slot_tag_(arr, len) = tag;
    *any_slot_val_(arr, len) = value;
    *(uint64_t *)((uint8_t *)arr + 8) = len + 1;
    /* Suppress unused-h warning when the realloc branch isn't taken. */
    (void)h;
    return arr;
}

/* P5.6 — extend dst with src's tagged slots. Both arrays are
 * Array<Any> layout (16-byte slots = u64 tag + u64 value). Each
 * appended slot's heap value (tag == __TORAJS_ANY_HEAP) gets its
 * refcount bumped so dst shares ownership; src retains its own
 * ownership unchanged. Returns the (possibly-realloc'd) dst ptr.
 * Bumps dst's len by src_len.
 *
 * Mirrors __torajs_arr_extend_unchecked for the regular 8-byte
 * slot path; that one didn't know about tagged slots and would
 * mis-stride a 16-byte source. Self-sizing — reallocs dst when
 * cap < required, doubling per the arr_push_any growth strategy.
 *
 * Caller MUST capture the return value and write it back to
 * whichever slot owns dst (matching arr_push / arr_unshift call
 * sites that thread the new ptr through). */
void *__torajs_arr_extend_any(uint8_t *dst, const uint8_t *src) {
    uint64_t dst_len = *(uint64_t *)((uint8_t *)dst + 8);
    uint64_t src_len = *(const uint64_t *)((const uint8_t *)src + 8);
    if (src_len == 0) return dst;
    uint32_t cap = *(uint32_t *)((uint8_t *)dst + 16);
    uint64_t needed = dst_len + src_len;
    if (needed > (uint64_t)cap) {
        uint32_t new_cap = cap == 0 ? 4 : cap;
        while ((uint64_t)new_cap < needed) new_cap *= 2;
        dst = (uint8_t *)realloc(
            dst,
            24 /* __TORAJS_ARR_HDR_SIZE */ + (size_t)new_cap * __TORAJS_ANY_SLOT_BYTES);
        *(uint32_t *)((uint8_t *)dst + 16) = new_cap;
    }
    for (uint64_t i = 0; i < src_len; i++) {
        uint64_t tag = *any_slot_tag_((void *)src, i);
        uint64_t val = *any_slot_val_((void *)src, i);
        if (tag == __TORAJS_ANY_HEAP && val != 0) {
            __torajs_rc_inc((void *)(uintptr_t)val);
        }
        *any_slot_tag_(dst, dst_len + i) = tag;
        *any_slot_val_(dst, dst_len + i) = val;
    }
    *(uint64_t *)((uint8_t *)dst + 8) = dst_len + src_len;
    return dst;
}

/* P1.4 — OOB read of an Array<Any> slot returns undefined per
 * ES spec §10.4.2.1 (sparse arrays return undefined for missing
 * indices). Pre-P1 the caller passed an out-of-range index
 * directly to any_slot_tag_/any_slot_val_ which read past the
 * cap (UB), often returning 0 by luck and getting collapsed to
 * ANY_NULL. Now both helpers do an explicit length check and
 * return ANY_UNDEF=5 / value 0 on miss; the typeof / strict-eq
 * paths then route through the spec-correct ANY_UNDEF behavior. */
uint64_t __torajs_arr_get_any_tag(const void *arr, uint64_t i) {
    if (arr == NULL) return 5 /* ANY_UNDEF */;
    uint64_t len = *(const uint64_t *)((const uint8_t *)arr + 8);
    if (i >= len) return 5 /* ANY_UNDEF */;
    return *any_slot_tag_((void *)arr, i);
}

uint64_t __torajs_arr_get_any_value(const void *arr, uint64_t i) {
    if (arr == NULL) return 0;
    uint64_t len = *(const uint64_t *)((const uint8_t *)arr + 8);
    if (i >= len) return 0;
    return *any_slot_val_((void *)arr, i);
}

/* Forward decl — definition lives further down in the file but
 * arr_set_any (immediately below) needs to call it for the heap-
 * value drop on slot overwrite. */
void __torajs_value_drop_heap(void *child);

/* P0.10 — set a tagged slot at index `i`. Mirrors arr_push_any
 * for the indexed-assign path. ssa_lower's box_to_any boxes the
 * RHS into a temp Any-box; we extract its (tag, value) and write
 * into the slot. ANY_HEAP slots: caller must have rc-incremented
 * the heap value before calling; we drop the old slot's heap
 * value (if ANY_HEAP) before overwriting so refcount accounting
 * stays balanced. NULL arr is a no-op. Out-of-bounds i is the
 * caller's responsibility (no bounds check, matching the existing
 * arr_get_any_* helpers). */
void __torajs_arr_set_any(void *arr, uint64_t i, uint64_t tag, uint64_t value) {
    if (arr == NULL) return;
    /* Drop old slot's heap value if it was ANY_HEAP (tag=4). */
    uint64_t old_tag = *any_slot_tag_(arr, i);
    if (old_tag == 4 /* ANY_HEAP */) {
        uint64_t old_val = *any_slot_val_(arr, i);
        __torajs_value_drop_heap((void *)(uintptr_t)old_val);
    }
    *any_slot_tag_(arr, i) = tag;
    *any_slot_val_(arr, i) = value;
}

/* P3.1 — Dynamic-property object substrate (HashMap-backed).
 *
 * Layout:
 *   offset 0  : __torajs_heap_header_t (8 bytes; refcount/tag/flags)
 *   offset 8  : count (u32)        — # of live entries
 *   offset 12 : cap   (u32)        — bucket array size (power of 2)
 *   offset 16 : tomb  (u32)        — # of tombstone slots
 *   offset 20 : pad   (u32)
 *   offset 24 : buckets[cap] of __torajs_dynobj_bucket_t (24 bytes each)
 *
 * Bucket:
 *   key_ptr : *Str    (NULL = empty; (void*)1 = tombstone; else owning Str ptr)
 *   tag     : u64     (ANY_NULL/UNDEF/BOOL/I64/F64/HEAP per existing scheme)
 *   value   : u64     (per-tag payload — bool/int/f64-bits/heap-ptr-as-u64)
 *
 * Probing: open addressing, linear probe step = 1.
 * Resize: at load factor (count + tomb) > cap * 7/8 — double cap.
 * Hash: FNV-1a over the key's bytes (Str layout: header + len + bytes).
 *
 * Reference: Swift Dictionary / CPython dict's compact open-addressing.
 * Self-implemented (no external lib) per CLAUDE.md "自研" pillar. */

#define __TORAJS_DYNOBJ_HDR_SIZE   24
#define __TORAJS_DYNOBJ_BUCKET_SIZE 24
#define __TORAJS_DYNOBJ_INITIAL_CAP 8  /* must be power of 2 */
#define __TORAJS_DYNOBJ_TOMBSTONE   ((void *)(uintptr_t)1)

/* P3.attribute-flag-tracking — pack attribute flags into bucket.tag
 * high bits. Low 8 bits stay ANY_TAG (0-5); bits 8-10 carry the spec
 * §6.2.5 PropertyDescriptor data-attribute flags. Avoids bucket
 * struct size growth (would have been 24 → 32 bytes = +33% memory
 * for every dynobj entry). */
#define __TORAJS_BUCKET_TAG_MASK         0xffULL
#define __TORAJS_BUCKET_FLAG_WRITABLE     (1ULL << 8)
#define __TORAJS_BUCKET_FLAG_ENUMERABLE   (1ULL << 9)
#define __TORAJS_BUCKET_FLAG_CONFIGURABLE (1ULL << 10)
/* Default flags for implicit set (`obj.x = v`) and object-literal
 * init: spec §10.1.5.1 OrdinarySet → §10.1.6.2 CreateDataProperty →
 * writable / enumerable / configurable all default to true. */
#define __TORAJS_BUCKET_FLAGS_DEFAULT \
    (__TORAJS_BUCKET_FLAG_WRITABLE \
     | __TORAJS_BUCKET_FLAG_ENUMERABLE \
     | __TORAJS_BUCKET_FLAG_CONFIGURABLE)

/* flags_byte encoding passed by ssa_lower's defineProperty intercept
 * to __torajs_dynobj_define. Low 3 bits = flag value; next 3 bits =
 * "flag present in descriptor". Spec §10.1.6.3 distinguishes "absent"
 * from "present-false": absent → leave current bucket's flag alone on
 * redefine (default-false on fresh insert); present → use the
 * specified value. */
#define __TORAJS_DEFINE_FLAG_WRITABLE      (1 << 0)
#define __TORAJS_DEFINE_FLAG_ENUMERABLE    (1 << 1)
#define __TORAJS_DEFINE_FLAG_CONFIGURABLE  (1 << 2)
#define __TORAJS_DEFINE_PRESENT_WRITABLE      (1 << 3)
#define __TORAJS_DEFINE_PRESENT_ENUMERABLE    (1 << 4)
#define __TORAJS_DEFINE_PRESENT_CONFIGURABLE  (1 << 5)
#define __TORAJS_DEFINE_PRESENT_VALUE         (1 << 6)

typedef struct {
    void *key_ptr;   /* owning Str* (rc'd); NULL = empty; TOMBSTONE = deleted */
    uint64_t tag;    /* low 8 bits = ANY_TAG; bits 8-10 = writable/enumerable/configurable */
    uint64_t value;
} __torajs_dynobj_bucket_t;

void __torajs_str_drop(void *s);

/* Forward decls used inside helpers. */
static __torajs_dynobj_bucket_t *__torajs_dynobj_buckets(void *obj);
static void __torajs_dynobj_resize(void **obj_slot, uint32_t new_cap);

/* FNV-1a over Str payload. Reads the Str layout directly:
 *   offset 0  : heap header (8)
 *   offset 8  : len (u64)
 *   offset 16 : utf-8 bytes
 */
static uint64_t __torajs_dynobj_hash_str(const void *key) {
    uint64_t h = 0xcbf29ce484222325ULL;
    uint64_t len = __TORAJS_STR_LEN(key);
    const uint8_t *data = __TORAJS_STR_CDATA(key);
    for (uint64_t i = 0; i < len; i++) {
        h ^= (uint64_t)data[i];
        h *= 0x100000001b3ULL;
    }
    return h;
}

/* Compare two Str values for equality (length + byte content). Used
 * by the bucket lookup probe — distinct Str pointers with the same
 * content must hit the same slot (per ES spec property-key equality).
 * Pointer-identity short-circuit (intern / interned-literal sites). */
static int __torajs_dynobj_str_eq(const void *a, const void *b) {
    if (a == b) return 1;
    uint64_t la = __TORAJS_STR_LEN(a);
    uint64_t lb = __TORAJS_STR_LEN(b);
    if (la != lb) return 0;
    return memcmp(__TORAJS_STR_CDATA(a), __TORAJS_STR_CDATA(b), (size_t)la) == 0;
}

static __torajs_dynobj_bucket_t *__torajs_dynobj_buckets(void *obj) {
    return (__torajs_dynobj_bucket_t *)((uint8_t *)obj + __TORAJS_DYNOBJ_HDR_SIZE);
}

void *__torajs_dynobj_alloc(void) {
    uint32_t cap = __TORAJS_DYNOBJ_INITIAL_CAP;
    size_t bytes = __TORAJS_DYNOBJ_HDR_SIZE
        + (size_t)cap * __TORAJS_DYNOBJ_BUCKET_SIZE;
    uint8_t *p = (uint8_t *)calloc(1, bytes);  /* zero-init = all empty buckets */
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_DYNOBJ;
    h->flags = 0;
    *(uint32_t *)(p + 8)  = 0;     /* count */
    *(uint32_t *)(p + 12) = cap;
    *(uint32_t *)(p + 16) = 0;     /* tomb */
    return p;
}

/* Probe for `key`. Returns the bucket index where:
 *   - the key is found, OR
 *   - an empty bucket (key_ptr == NULL) is reachable for insertion.
 * If the probe finds a tombstone first, remember it and use it for
 * insertion if the key is ultimately not found. Caller distinguishes
 * found vs not-found via `*out_found`. */
static uint32_t __torajs_dynobj_probe(
    const void *obj, const void *key, int *out_found
) {
    uint32_t cap = *(const uint32_t *)((const uint8_t *)obj + 12);
    __torajs_dynobj_bucket_t *bk =
        (__torajs_dynobj_bucket_t *)((uint8_t *)(uintptr_t)obj + __TORAJS_DYNOBJ_HDR_SIZE);
    uint64_t h = __torajs_dynobj_hash_str(key);
    uint32_t mask = cap - 1;
    uint32_t i = (uint32_t)(h & mask);
    int32_t tombstone_at = -1;
    for (uint32_t step = 0; step < cap; step++) {
        uint32_t idx = (i + step) & mask;
        void *kp = bk[idx].key_ptr;
        if (kp == NULL) {
            *out_found = 0;
            return tombstone_at >= 0 ? (uint32_t)tombstone_at : idx;
        }
        if (kp == __TORAJS_DYNOBJ_TOMBSTONE) {
            if (tombstone_at < 0) tombstone_at = (int32_t)idx;
            continue;
        }
        if (__torajs_dynobj_str_eq(kp, key)) {
            *out_found = 1;
            return idx;
        }
    }
    /* Should never reach (resize keeps load factor < 1). */
    *out_found = 0;
    return tombstone_at >= 0 ? (uint32_t)tombstone_at : 0;
}

static void __torajs_dynobj_resize(void **obj_slot, uint32_t new_cap) {
    void *old = *obj_slot;
    uint32_t old_cap = *(uint32_t *)((uint8_t *)old + 12);
    __torajs_dynobj_bucket_t *old_bk = __torajs_dynobj_buckets(old);
    /* Allocate fresh block with new_cap. */
    size_t bytes = __TORAJS_DYNOBJ_HDR_SIZE
        + (size_t)new_cap * __TORAJS_DYNOBJ_BUCKET_SIZE;
    uint8_t *p = (uint8_t *)calloc(1, bytes);
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    *h = *(__torajs_heap_header_t *)old;  /* preserve refcount + tag + flags */
    *(uint32_t *)(p + 8)  = 0;          /* count rebuilds below */
    *(uint32_t *)(p + 12) = new_cap;
    *(uint32_t *)(p + 16) = 0;          /* no tombstones in fresh table */
    void *new_obj = p;
    /* Re-insert every live bucket into the new table. Tombstones drop. */
    uint32_t live = 0;
    for (uint32_t i = 0; i < old_cap; i++) {
        void *kp = old_bk[i].key_ptr;
        if (kp == NULL || kp == __TORAJS_DYNOBJ_TOMBSTONE) continue;
        int found;
        uint32_t idx = __torajs_dynobj_probe(new_obj, kp, &found);
        __torajs_dynobj_bucket_t *new_bk = __torajs_dynobj_buckets(new_obj);
        new_bk[idx] = old_bk[i];
        live++;
    }
    *(uint32_t *)((uint8_t *)new_obj + 8) = live;
    *obj_slot = new_obj;
    free(old);
}

/* Get tag for `key`. Returns ANY_UNDEF=5 when the key isn't present
 * (per ES spec — missing property reads as undefined). Also returns
 * ANY_UNDEF when `obj` is not a dynobj (e.g. a typed Struct passed
 * via Any-box from `obj?.field.subfield` chained access). Without
 * this defensive check, dynobj_probe would index into the wrong
 * layout and return garbage tag values silently. */
uint64_t __torajs_dynobj_get_tag(const void *obj, const void *key) {
    if (obj == NULL) return 5;
    const __torajs_heap_header_t *h = (const __torajs_heap_header_t *)obj;
    if (h->type_tag != __TORAJS_TAG_DYNOBJ) return 5;  /* ANY_UNDEF */
    int found;
    uint32_t idx = __torajs_dynobj_probe(obj, key, &found);
    if (!found) return 5;  /* ANY_UNDEF */
    /* P3.attribute-flag-tracking — mask out the high-bit attribute
     * flags; callers expect raw ANY_TAG (0-5) for value unboxing. */
    return __torajs_dynobj_buckets((void *)(uintptr_t)obj)[idx].tag
        & __TORAJS_BUCKET_TAG_MASK;
}

uint64_t __torajs_dynobj_get_value(const void *obj, const void *key) {
    if (obj == NULL) return 0;
    const __torajs_heap_header_t *h = (const __torajs_heap_header_t *)obj;
    if (h->type_tag != __TORAJS_TAG_DYNOBJ) return 0;
    int found;
    uint32_t idx = __torajs_dynobj_probe(obj, key, &found);
    if (!found) return 0;
    return __torajs_dynobj_buckets((void *)(uintptr_t)obj)[idx].value;
}

/* P3.getOwnPropertyDescriptor — return the bucket's attribute flags
 * packed as bit 0 = writable, bit 1 = enumerable, bit 2 = configurable.
 * Caller extracts each bit to populate the descriptor object's
 * boolean fields. Returns 0 when key is absent. */
uint64_t __torajs_dynobj_get_flags(const void *obj, const void *key) {
    if (obj == NULL) return 0;
    const __torajs_heap_header_t *h = (const __torajs_heap_header_t *)obj;
    if (h->type_tag != __TORAJS_TAG_DYNOBJ) return 0;
    int found;
    uint32_t idx = __torajs_dynobj_probe(obj, key, &found);
    if (!found) return 0;
    uint64_t t = __torajs_dynobj_buckets((void *)(uintptr_t)obj)[idx].tag;
    uint64_t flags = 0;
    if (t & __TORAJS_BUCKET_FLAG_WRITABLE)     flags |= 1ULL << 0;
    if (t & __TORAJS_BUCKET_FLAG_ENUMERABLE)   flags |= 1ULL << 1;
    if (t & __TORAJS_BUCKET_FLAG_CONFIGURABLE) flags |= 1ULL << 2;
    return flags;
}

/* P3.getOwnPropertyDescriptor — full ES spec §19.1.2.10 entry. Takes
 * an Any-box (must wrap a dynobj) + a string key; returns a fresh
 * Any-box wrapping either:
 *   - A new dynobj with the four data-descriptor fields
 *     {value, writable, enumerable, configurable} (when the key is
 *     present); or
 *   - ANY_UNDEF (when the key is absent or the box doesn't wrap a
 *     dynobj — spec §19.1.2.10 step 1 ToObject coercion / step 4
 *     `if Type(P) is String/Symbol then ToPropertyKey...`).
 *
 * Builtin obj-shape descriptors (Array.length etc.) are still a
 * follow-up — those need bespoke shape construction per-builtin. */

/* Forward decls — defined further down in the file. The Any-box
 * helpers + offset constants live near the BinOp Any path (~line
 * 1520-1600); the dynobj_has helper sits above dynobj_set in this
 * file but the explicit forward decls keep us robust to future
 * reorderings. */
extern void *__torajs_any_box(int64_t tag, int64_t value);
extern int __torajs_dynobj_has(const void *obj, const void *key);
extern void __torajs_value_drop_heap(void *child);
void __torajs_dynobj_set(void **obj_slot, void *key, uint64_t tag, uint64_t value);
#define __TORAJS_ANY_BOX_TAG_OFF 8
#define __TORAJS_ANY_BOX_VAL_OFF 16
void *__torajs_get_property_descriptor(void *obj_any, void *key) {
    if (obj_any == NULL || key == NULL) {
        return __torajs_any_box(__TORAJS_ANY_UNDEF, 0);
    }
    int64_t obj_tag = *(int64_t *)((uint8_t *)obj_any + __TORAJS_ANY_BOX_TAG_OFF);
    if (obj_tag != __TORAJS_ANY_HEAP) {
        return __torajs_any_box(__TORAJS_ANY_UNDEF, 0);
    }
    void *dynobj = (void *)(uintptr_t)
        *(int64_t *)((uint8_t *)obj_any + __TORAJS_ANY_BOX_VAL_OFF);
    if (dynobj == NULL) {
        return __torajs_any_box(__TORAJS_ANY_UNDEF, 0);
    }
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)dynobj;
    if (h->type_tag != __TORAJS_TAG_DYNOBJ) {
        return __torajs_any_box(__TORAJS_ANY_UNDEF, 0);
    }
    if (!__torajs_dynobj_has(dynobj, key)) {
        return __torajs_any_box(__TORAJS_ANY_UNDEF, 0);
    }

    uint64_t v_tag = __torajs_dynobj_get_tag(dynobj, key);
    uint64_t v_val = __torajs_dynobj_get_value(dynobj, key);
    uint64_t flags = __torajs_dynobj_get_flags(dynobj, key);

    void *desc = __torajs_dynobj_alloc();

    /* Define the 4 descriptor fields. ANY_HEAP value needs an rc bump
     * so the new dynobj owns its share independently of the source. */
    static const char *const k_names[4] = {
        "value", "writable", "enumerable", "configurable",
    };
    static const uint64_t k_lens[4] = { 5, 8, 10, 12 };
    uint64_t k_tags[4]  = { v_tag, __TORAJS_ANY_BOOL, __TORAJS_ANY_BOOL, __TORAJS_ANY_BOOL };
    uint64_t k_vals[4]  = {
        v_val,
        (flags >> 0) & 1,
        (flags >> 1) & 1,
        (flags >> 2) & 1,
    };
    if (v_tag == __TORAJS_ANY_HEAP) {
        __torajs_rc_inc((void *)(uintptr_t)v_val);
    }
    for (int i = 0; i < 4; i++) {
        uint8_t *k = __torajs_str_alloc_pooled(k_lens[i]);
        memcpy(__TORAJS_STR_DATA(k), k_names[i], (size_t)k_lens[i]);
        __torajs_dynobj_set(&desc, k, k_tags[i], k_vals[i]);
        __torajs_str_drop(k);
    }

    void *result = __torajs_any_box(__TORAJS_ANY_HEAP, (int64_t)(uintptr_t)desc);
    /* any_box rc_inc'd desc (refcount now 2: our local + the box).
     * Drop our local so the box becomes the sole owner. */
    __torajs_value_drop_heap(desc);
    return result;
}

/* Forward decl — __torajs_throw_set is still LLVM-IR-emitted by
 * ssa_inkwell (moves to Rust in P2.4-b). */
extern void __torajs_throw_set(int64_t tag, int64_t value);
/* P2.4-a — native-error registry + throw_range/type_error
 * cross-TU wrappers now provided by the Rust `torajs-throw` crate.
 * Forward-declared here so the in-file C callers below
 * (dynobj_set / dynobj_define / arr_set_length_validate / etc.)
 * resolve them at link time. */
extern void __torajs_register_native_error(int64_t slot, void *fnptr);
extern void __torajs_throw_range_error(const char *msg);
extern void __torajs_throw_type_error(const char *msg);

/* Set `obj[key] = (tag, value)`. Caller is responsible for rc-bumping
 * heap-typed values BEFORE calling (matches arr_push_any contract).
 * The key is borrowed if it's already present; rc-bumped if it's a
 * fresh insert (so the bucket owns its share).
 *
 * P3.attribute-flag-tracking — implicit set (`obj.x = v` and
 * object-literal init) now honors the writable flag on existing
 * buckets. New buckets default to all-true flags (spec §10.1.6.2
 * CreateDataProperty). Existing-bucket overwrite preserves flag bits
 * and only updates the ANY_TAG low bits + value. Writable=false →
 * `__torajs_throw_set` with TypeError + return without mutation
 * (caller's ssa_lower-side `emit_throw_check` propagates). */
void __torajs_dynobj_set(void **obj_slot, void *key, uint64_t tag, uint64_t value) {
    void *obj = *obj_slot;
    if (obj == NULL) return;
    uint32_t cap = *(uint32_t *)((uint8_t *)obj + 12);
    uint32_t count = *(uint32_t *)((uint8_t *)obj + 8);
    uint32_t tomb = *(uint32_t *)((uint8_t *)obj + 16);
    /* Resize before insert if (count + tomb + 1) > cap * 7/8 to keep
     * load factor under control. */
    if ((count + tomb + 1) * 8 > cap * 7) {
        __torajs_dynobj_resize(obj_slot, cap * 2);
        obj = *obj_slot;
    }
    int found;
    uint32_t idx = __torajs_dynobj_probe(obj, key, &found);
    __torajs_dynobj_bucket_t *bk = __torajs_dynobj_buckets(obj);
    if (found) {
        uint64_t cur_tag = bk[idx].tag;
        if (!(cur_tag & __TORAJS_BUCKET_FLAG_WRITABLE)) {
            __torajs_throw_type_error(
                "TypeError: Cannot assign to read only property");
            return;
        }
        /* Drop old heap value if it was ANY_HEAP. Mask to read just
         * the ANY_TAG low bits. */
        if ((cur_tag & __TORAJS_BUCKET_TAG_MASK) == 4 /* ANY_HEAP */) {
            void *old_val = (void *)(uintptr_t)bk[idx].value;
            __torajs_value_drop_heap(old_val);
        }
        /* Preserve existing flag bits; only swap the value-type tag. */
        bk[idx].tag = (cur_tag & ~__TORAJS_BUCKET_TAG_MASK)
            | (tag & __TORAJS_BUCKET_TAG_MASK);
        bk[idx].value = value;
    } else {
        /* Fresh insert. Take ownership of key (rc-bump). Default
         * flags to all-true (implicit-set / object-literal-init
         * spec semantics). */
        if (bk[idx].key_ptr == __TORAJS_DYNOBJ_TOMBSTONE) {
            *(uint32_t *)((uint8_t *)obj + 16) = tomb - 1;
        }
        __torajs_rc_inc(key);
        bk[idx].key_ptr = key;
        bk[idx].tag = (tag & __TORAJS_BUCKET_TAG_MASK)
            | __TORAJS_BUCKET_FLAGS_DEFAULT;
        bk[idx].value = value;
        *(uint32_t *)((uint8_t *)obj + 8) = count + 1;
    }
}

/* P3.attribute-flag-tracking — `Object.defineProperty(obj, key,
 * descriptor)` path. Implements spec §10.1.6.3 ValidateAndApply-
 * PropertyDescriptor data-property subset:
 *
 * - If bucket is fresh (no current property): write the new bucket
 *   with the flags from `flags_byte` (absent flags default to false
 *   per spec §10.1.6.2).
 * - If bucket exists (redefine):
 *   - current.configurable=false: throw if desc tries to upgrade
 *     configurable to true, change enumerable, or upgrade writable
 *     from false→true, or change value while writable=false.
 *   - else: apply the present-flag updates, preserve the rest.
 *
 * `flags_byte` encodes (low byte):
 *   bit 0/1/2 = writable / enumerable / configurable VALUE
 *   bit 3/4/5 = writable / enumerable / configurable PRESENT
 *   bit 6     = value PRESENT in descriptor
 *
 * tag / value: the descriptor's [[Value]] packed as ANY_TAG. Ignored
 * if `value PRESENT` bit is clear.
 *
 * On spec violation: __torajs_throw_set with TypeError and return
 * without mutating bucket. ssa_lower's emit_throw_check propagates. */
void __torajs_dynobj_define(
    void **obj_slot,
    void *key,
    uint64_t tag,
    uint64_t value,
    uint64_t flags_byte
) {
    void *obj = *obj_slot;
    if (obj == NULL) return;
    uint32_t cap = *(uint32_t *)((uint8_t *)obj + 12);
    uint32_t count = *(uint32_t *)((uint8_t *)obj + 8);
    uint32_t tomb = *(uint32_t *)((uint8_t *)obj + 16);
    if ((count + tomb + 1) * 8 > cap * 7) {
        __torajs_dynobj_resize(obj_slot, cap * 2);
        obj = *obj_slot;
    }
    int found;
    uint32_t idx = __torajs_dynobj_probe(obj, key, &found);
    __torajs_dynobj_bucket_t *bk = __torajs_dynobj_buckets(obj);

    int has_writable     = (flags_byte & __TORAJS_DEFINE_PRESENT_WRITABLE) != 0;
    int has_enumerable   = (flags_byte & __TORAJS_DEFINE_PRESENT_ENUMERABLE) != 0;
    int has_configurable = (flags_byte & __TORAJS_DEFINE_PRESENT_CONFIGURABLE) != 0;
    int has_value        = (flags_byte & __TORAJS_DEFINE_PRESENT_VALUE) != 0;
    int desc_writable     = (flags_byte & __TORAJS_DEFINE_FLAG_WRITABLE) != 0;
    int desc_enumerable   = (flags_byte & __TORAJS_DEFINE_FLAG_ENUMERABLE) != 0;
    int desc_configurable = (flags_byte & __TORAJS_DEFINE_FLAG_CONFIGURABLE) != 0;

    if (found) {
        uint64_t cur_tag = bk[idx].tag;
        int cur_writable     = (cur_tag & __TORAJS_BUCKET_FLAG_WRITABLE) != 0;
        int cur_enumerable   = (cur_tag & __TORAJS_BUCKET_FLAG_ENUMERABLE) != 0;
        int cur_configurable = (cur_tag & __TORAJS_BUCKET_FLAG_CONFIGURABLE) != 0;
        uint64_t cur_value_tag = cur_tag & __TORAJS_BUCKET_TAG_MASK;

        if (!cur_configurable) {
            /* Spec §10.1.6.3: with current.configurable=false, any
             * present-flag change that diverges from current throws. */
            if (has_configurable && desc_configurable && !cur_configurable) {
                /* Trying to flip false → true. */
                __torajs_throw_type_error(
                    "TypeError: Cannot redefine property: configurable was false");
                return;
            }
            if (has_enumerable && desc_enumerable != cur_enumerable) {
                __torajs_throw_type_error(
                    "TypeError: Cannot redefine property: enumerable mismatch");
                return;
            }
            if (!cur_writable) {
                if (has_writable && desc_writable) {
                    /* Cannot upgrade writable false → true under
                     * non-configurable. */
                    __torajs_throw_type_error(
                        "TypeError: Cannot redefine property: writable was false");
                    return;
                }
                if (has_value) {
                    /* Spec: with writable=false + non-configurable,
                     * value changes are rejected unless the new value
                     * is SameValue with the old. SameValue here is
                     * approximated by exact (tag,value) match — same
                     * heuristic used by the BinOp Any===Any arm. */
                    int same = ((tag & __TORAJS_BUCKET_TAG_MASK) == cur_value_tag)
                        && (value == bk[idx].value);
                    if (!same) {
                        __torajs_throw_type_error(
                            "TypeError: Cannot redefine property: writable was false, value mismatch");
                        return;
                    }
                }
            }
        }

        /* Validation passed — apply the update. Drop old heap value
         * if we're overwriting an ANY_HEAP slot. */
        if (has_value
            && cur_value_tag == 4 /* ANY_HEAP */)
        {
            void *old_val = (void *)(uintptr_t)bk[idx].value;
            __torajs_value_drop_heap(old_val);
        }

        /* Recompute flag bits: present-flag overrides current; absent
         * preserves. */
        uint64_t new_flags = 0;
        new_flags |= (has_writable ? (desc_writable ? __TORAJS_BUCKET_FLAG_WRITABLE : 0)
                                   : (cur_writable ? __TORAJS_BUCKET_FLAG_WRITABLE : 0));
        new_flags |= (has_enumerable ? (desc_enumerable ? __TORAJS_BUCKET_FLAG_ENUMERABLE : 0)
                                     : (cur_enumerable ? __TORAJS_BUCKET_FLAG_ENUMERABLE : 0));
        new_flags |= (has_configurable ? (desc_configurable ? __TORAJS_BUCKET_FLAG_CONFIGURABLE : 0)
                                       : (cur_configurable ? __TORAJS_BUCKET_FLAG_CONFIGURABLE : 0));

        uint64_t new_value_tag = has_value ? (tag & __TORAJS_BUCKET_TAG_MASK) : cur_value_tag;
        uint64_t new_value     = has_value ? value : bk[idx].value;

        bk[idx].tag = new_value_tag | new_flags;
        bk[idx].value = new_value;
    } else {
        /* Fresh define. Absent flags default to false (spec
         * §10.1.6.2). */
        if (bk[idx].key_ptr == __TORAJS_DYNOBJ_TOMBSTONE) {
            *(uint32_t *)((uint8_t *)obj + 16) = tomb - 1;
        }
        __torajs_rc_inc(key);
        uint64_t new_flags = 0;
        if (desc_writable)     new_flags |= __TORAJS_BUCKET_FLAG_WRITABLE;
        if (desc_enumerable)   new_flags |= __TORAJS_BUCKET_FLAG_ENUMERABLE;
        if (desc_configurable) new_flags |= __TORAJS_BUCKET_FLAG_CONFIGURABLE;
        bk[idx].key_ptr = key;
        /* If no .value present, store ANY_UNDEF=5 + 0 (spec default
         * for new data descriptor's [[Value]]). */
        if (has_value) {
            bk[idx].tag = (tag & __TORAJS_BUCKET_TAG_MASK) | new_flags;
            bk[idx].value = value;
        } else {
            bk[idx].tag = 5 /* ANY_UNDEF */ | new_flags;
            bk[idx].value = 0;
        }
        *(uint32_t *)((uint8_t *)obj + 8) = count + 1;
    }
}

int __torajs_dynobj_has(const void *obj, const void *key) {
    if (obj == NULL) return 0;
    int found;
    (void)__torajs_dynobj_probe(obj, key, &found);
    return found;
}

int __torajs_dynobj_delete(void *obj, const void *key) {
    if (obj == NULL) return 0;
    int found;
    uint32_t idx = __torajs_dynobj_probe(obj, key, &found);
    if (!found) return 0;
    __torajs_dynobj_bucket_t *bk = __torajs_dynobj_buckets(obj);
    /* Drop key + value (heap if ANY_HEAP). */
    __torajs_str_drop(bk[idx].key_ptr);
    if ((bk[idx].tag & __TORAJS_BUCKET_TAG_MASK) == 4 /* ANY_HEAP */) {
        __torajs_value_drop_heap((void *)(uintptr_t)bk[idx].value);
    }
    bk[idx].key_ptr = __TORAJS_DYNOBJ_TOMBSTONE;
    bk[idx].tag = 0;
    bk[idx].value = 0;
    uint32_t count = *(uint32_t *)((uint8_t *)obj + 8);
    uint32_t tomb = *(uint32_t *)((uint8_t *)obj + 16);
    *(uint32_t *)((uint8_t *)obj + 8) = count - 1;
    *(uint32_t *)((uint8_t *)obj + 16) = tomb + 1;
    return 1;
}

/* Drop a dynobj. Walks every live bucket, drops the key Str and any
 * ANY_HEAP value, then frees the block. Called via universal value-
 * drop dispatch when the dynobj's refcount hits zero. */
void __torajs_dynobj_drop(void *obj) {
    if (obj == NULL) return;
    if (!__torajs_rc_dec(obj)) return;
    uint32_t cap = *(uint32_t *)((uint8_t *)obj + 12);
    __torajs_dynobj_bucket_t *bk = __torajs_dynobj_buckets(obj);
    for (uint32_t i = 0; i < cap; i++) {
        void *kp = bk[i].key_ptr;
        if (kp == NULL || kp == __TORAJS_DYNOBJ_TOMBSTONE) continue;
        __torajs_str_drop(kp);
        if ((bk[i].tag & __TORAJS_BUCKET_TAG_MASK) == 4 /* ANY_HEAP */) {
            __torajs_value_drop_heap((void *)(uintptr_t)bk[i].value);
        }
    }
    free(obj);
}

/* Forward decls for the inkwell-emitted *_drop helpers. They live
 * in the AOT binary's IR module; cc -c sees them via the linker
 * after the link step, so an implicit-function-declaration warning
 * here is harmless but noisy. Forward-decling keeps the C runtime
 * tidy + avoids -Wint-conversion on call sites. */
void __torajs_str_drop(void *s);
void __torajs_arr_drop(void *a);

/* Universal heap-typed value drop. Reads the value's `type_tag`
 * and routes to the matching `__torajs_*_drop` (which itself does
 * rc_dec + per-type free at zero). Used by Any-box drop and by
 * Array<Any> slot walk to release ANY_HEAP-tagged children. NULL
 * input is a no-op. T-10.d.i covers Str + Arr; Obj / Substr /
 * Closure / RegExp / Date land as the corresponding `*_drop`s
 * acquire C linkage — for now those tags fall back to `free()`
 * which is leak-safe (frees the outer block; misses inner
 * refcounted fields). T-10.e tightens the dispatch. */
#ifndef __wasi__
extern void __torajs_response_drop(void *p);
#endif
extern void __torajs_bigint_drop(void *p);
extern void __torajs_weakref_drop(void *p);
extern void __torajs_weakmap_drop(void *p);
extern void __torajs_weakset_drop(void *p);
extern void __torajs_map_drop(void *p);
extern void __torajs_map_iter_drop(void *p);
extern void __torajs_arr_iter_drop(void *p);

void __torajs_value_drop_heap(void *child) {
    if (child == NULL) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)child;
    switch (h->type_tag) {
        case __TORAJS_TAG_STR:  __torajs_str_drop(child); break;
        case __TORAJS_TAG_ARR:  __torajs_arr_drop(child); break;
#ifndef __wasi__
        case __TORAJS_TAG_RESPONSE: __torajs_response_drop(child); break;
#endif
        case __TORAJS_TAG_BIGINT: {
            /* T-25 — BigInt has no inner refs; rc-dec + the
             * type's drop handle (which just `free`s). */
            if (__torajs_rc_dec(child)) __torajs_bigint_drop(child);
            break;
        }
        case __TORAJS_TAG_WEAKREF: {
            /* T-26 — WeakRef holds no strong ref to target;
             * weakref_drop dec's its rc + unregisters from the
             * registry on last owner. */
            __torajs_weakref_drop(child);
            break;
        }
        case __TORAJS_TAG_WEAKMAP: {
            /* T-26.B — WeakMap drop walks every entry, drops
             * each value's strong ref + deregisters the key
             * from the shared registry. */
            __torajs_weakmap_drop(child);
            break;
        }
        case __TORAJS_TAG_WEAKSET: {
            __torajs_weakset_drop(child);
            break;
        }
        case __TORAJS_TAG_MAP: {
            /* P6.1 / P6.2 — Map and Set both wear TAG_MAP at the
             * heap-header level (Type::Set is a SSA-side
             * distinction; storage is the same). map_drop walks
             * live entries, drops each (key, value) heap ref, and
             * frees the entries array + Map struct. */
            __torajs_map_drop(child);
            break;
        }
        case __TORAJS_TAG_MAP_ITER: {
            /* P6.4b — MapIter holds a strong ref to the source
             * Map; map_iter_drop releases it + frees the iter. */
            __torajs_map_iter_drop(child);
            break;
        }
        case __TORAJS_TAG_ARR_ITER: {
            /* P6.4c-C3 — ArrIter parallel to MapIter for
             * Array<Any> source. */
            __torajs_arr_iter_drop(child);
            break;
        }
        case __TORAJS_TAG_DYNOBJ: {
            /* P3.1 — dynobj walks every live bucket, drops key Str
             * + ANY_HEAP value, then frees. */
            __torajs_dynobj_drop(child);
            break;
        }
        default:                /* Obj / Substr / Closure / RegExp /
                                 * Date / ANY_BOX — fallback rc_dec +
                                 * free; may leak inner refs.
                                 *
                                 * V3-10.b: array element walks for
                                 * Type::Obj go through emit_drop_value
                                 * → inline drop → obj_drop (which
                                 * cycle-unbuffers), so cycle-routed
                                 * class instances are scrubbed without
                                 * needing the hook here. Non-Obj heap
                                 * children handled by their own _drop. */
            if (__torajs_rc_dec(child)) free(child);
            break;
    }
}

/* Drop an Array<Any>. rc-aware: dec, return early if shared.
 * On last-owner (rc hit 0), walks every slot — for ANY_HEAP slots
 * routes through `__torajs_value_drop_heap` so the child's per-type
 * free runs. Then frees the outer block. Mirrors regular arr_drop's
 * rc-awareness so emit_drop_value Type::Arr(Any) can call this once
 * per scope-exit without manual rc bookkeeping. */
void __torajs_arrprops_drop_entry(void *arr_ptr); /* fwd decl */

void __torajs_arr_drop_any(void *arr) {
    if (arr == NULL) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)arr;
    if (h->flags & __TORAJS_FLAG_STATIC_LITERAL) return;
    if (!__torajs_rc_dec(arr)) return; /* shared, keep alive */
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    for (uint64_t i = 0; i < len; i++) {
        uint64_t tag = *any_slot_tag_(arr, i);
        if (tag == __TORAJS_ANY_HEAP) {
            void *child = (void *)(uintptr_t)*any_slot_val_(arr, i);
            __torajs_value_drop_heap(child);
        }
    }
    /* T-29 — drop side-table props entry (no-op for arrays without
     * `arr.x = v` written). */
    __torajs_arrprops_drop_entry(arr);
    free(arr);
}

/* T-27.b — Function-as-Object side table for top-level FnDecls.
 *
 * Top-level FnDecls (Type::FnSig at SSA layer) are bare fn pointers
 * with no env block, so the in-closure props_dynobj field at
 * CLOSURE_PROPS_OFF doesn't apply. Instead we keep a global
 * hashmap keyed by fn pointer → dynobj. Top-level FnDecls live
 * forever (no drop), so no cleanup hook needed.
 *
 * Hash: 256 buckets, MurmurHash-style finalizer mix on the
 * pointer's bits. Chained collision resolution. Per-fn allocs
 * happen lazily on first prop access — fns that never get
 * `.x = v` pay zero cost (no node, no dynobj).
 *
 * Closure-form fns (Type::Closure) use the in-layout path
 * (CLOSURE_PROPS_OFF) — see fn_props_set / fn_props_get in
 * ssa_lower.rs. Side-table is only for FnSig. */

typedef struct __torajs_fnprops_node {
    void *fn_ptr;
    void *dynobj;
    struct __torajs_fnprops_node *next;
} __torajs_fnprops_node_t;

#define __TORAJS_FNPROPS_BUCKETS 256
static __torajs_fnprops_node_t *__torajs_fnprops_table[__TORAJS_FNPROPS_BUCKETS] = {0};

static uint32_t __torajs_fnprops_hash(void *p) {
    uintptr_t x = (uintptr_t)p;
    x = (x ^ (x >> 33)) * 0xff51afd7ed558ccdULL;
    x = (x ^ (x >> 33)) * 0xc4ceb9fe1a85ec53ULL;
    x = x ^ (x >> 33);
    return (uint32_t)(x % __TORAJS_FNPROPS_BUCKETS);
}

static __torajs_fnprops_node_t *__torajs_fnprops_find(void *fn_ptr) {
    uint32_t h = __torajs_fnprops_hash(fn_ptr);
    __torajs_fnprops_node_t *n = __torajs_fnprops_table[h];
    while (n) {
        if (n->fn_ptr == fn_ptr) return n;
        n = n->next;
    }
    return NULL;
}

static __torajs_fnprops_node_t *__torajs_fnprops_intern(void *fn_ptr) {
    __torajs_fnprops_node_t *n = __torajs_fnprops_find(fn_ptr);
    if (n) return n;
    uint32_t h = __torajs_fnprops_hash(fn_ptr);
    n = (__torajs_fnprops_node_t *)malloc(sizeof(__torajs_fnprops_node_t));
    n->fn_ptr = fn_ptr;
    n->dynobj = NULL;
    n->next = __torajs_fnprops_table[h];
    __torajs_fnprops_table[h] = n;
    return n;
}

void __torajs_fnprops_set(void *fn_ptr, void *key, int64_t tag, int64_t value) {
    __torajs_fnprops_node_t *n = __torajs_fnprops_intern(fn_ptr);
    if (n->dynobj == NULL) n->dynobj = __torajs_dynobj_alloc();
    __torajs_dynobj_set(&n->dynobj, key, (uint64_t)tag, (uint64_t)value);
}

uint64_t __torajs_fnprops_get_tag(void *fn_ptr, const void *key) {
    __torajs_fnprops_node_t *n = __torajs_fnprops_find(fn_ptr);
    if (n == NULL || n->dynobj == NULL) return 5;  /* ANY_UNDEF */
    return __torajs_dynobj_get_tag(n->dynobj, key);
}

uint64_t __torajs_fnprops_get_value(void *fn_ptr, const void *key) {
    __torajs_fnprops_node_t *n = __torajs_fnprops_find(fn_ptr);
    if (n == NULL || n->dynobj == NULL) return 0;
    return __torajs_dynobj_get_value(n->dynobj, key);
}

/* T-29 — Array-as-Object side table. Per ECMAScript, Array values are
 * Objects: `arr.x = v` and `arr.x` are spec-required. Pre-T-29 tora
 * rejected with "field assignment target must be a struct, got
 * Array(...)".
 *
 * Same shape as fnprops side table but distinct so the array-drop
 * hook only walks array entries (and avoids touching FnSig entries
 * which never drop). 256 buckets, MurmurHash finalizer mix on the
 * pointer's bits.
 *
 * The drop hook (called from arr_drop / arr_drop_any when refcount
 * hits 0) walks the bucket, drops the dynobj if non-NULL, removes
 * the node from the chain, and frees it. Arrays that never have
 * `.x = v` written cost zero (no node, no dynobj).
 *
 * Layout-extension alternative would put the props slot in the
 * Array header directly (mirroring CLOSURE_PROPS_OFF for Closure)
 * but Array is a much higher-traffic type — every array access
 * site reads ARR_DATA_OFF, and shifting that constant breaks ~28
 * call sites across runtime + ssa_lower. Side-table contains the
 * change to this file + ssa_lower's Member-on-Array branch +
 * arr_drop's hook call. */

typedef struct __torajs_arrprops_node {
    void *arr_ptr;
    void *dynobj;
    struct __torajs_arrprops_node *next;
} __torajs_arrprops_node_t;

#define __TORAJS_ARRPROPS_BUCKETS 256
static __torajs_arrprops_node_t *__torajs_arrprops_table[__TORAJS_ARRPROPS_BUCKETS] = {0};

static uint32_t __torajs_arrprops_hash(void *p) {
    uintptr_t x = (uintptr_t)p;
    x = (x ^ (x >> 33)) * 0xff51afd7ed558ccdULL;
    x = (x ^ (x >> 33)) * 0xc4ceb9fe1a85ec53ULL;
    x = x ^ (x >> 33);
    return (uint32_t)(x % __TORAJS_ARRPROPS_BUCKETS);
}

static __torajs_arrprops_node_t *__torajs_arrprops_find(void *arr_ptr) {
    uint32_t h = __torajs_arrprops_hash(arr_ptr);
    __torajs_arrprops_node_t *n = __torajs_arrprops_table[h];
    while (n) {
        if (n->arr_ptr == arr_ptr) return n;
        n = n->next;
    }
    return NULL;
}

static __torajs_arrprops_node_t *__torajs_arrprops_intern(void *arr_ptr) {
    __torajs_arrprops_node_t *n = __torajs_arrprops_find(arr_ptr);
    if (n) return n;
    uint32_t h = __torajs_arrprops_hash(arr_ptr);
    n = (__torajs_arrprops_node_t *)malloc(sizeof(__torajs_arrprops_node_t));
    n->arr_ptr = arr_ptr;
    n->dynobj = NULL;
    n->next = __torajs_arrprops_table[h];
    __torajs_arrprops_table[h] = n;
    return n;
}

void __torajs_arrprops_set(void *arr_ptr, void *key, int64_t tag, int64_t value) {
    __torajs_arrprops_node_t *n = __torajs_arrprops_intern(arr_ptr);
    if (n->dynobj == NULL) n->dynobj = __torajs_dynobj_alloc();
    __torajs_dynobj_set(&n->dynobj, key, (uint64_t)tag, (uint64_t)value);
}

uint64_t __torajs_arrprops_get_tag(void *arr_ptr, const void *key) {
    __torajs_arrprops_node_t *n = __torajs_arrprops_find(arr_ptr);
    if (n == NULL || n->dynobj == NULL) return 5;  /* ANY_UNDEF */
    return __torajs_dynobj_get_tag(n->dynobj, key);
}

uint64_t __torajs_arrprops_get_value(void *arr_ptr, const void *key) {
    __torajs_arrprops_node_t *n = __torajs_arrprops_find(arr_ptr);
    if (n == NULL || n->dynobj == NULL) return 0;
    return __torajs_dynobj_get_value(n->dynobj, key);
}

/* Drop hook called from arr_drop / arr_drop_any when an array's
 * refcount hits 0. Walks the bucket chain, removes + frees the
 * matching node + dec's the dynobj's refcount (which falls through
 * dynobj_drop's free walk). Common case (array never had props
 * written): bucket walk finds nothing, no-op. */
void __torajs_arrprops_drop_entry(void *arr_ptr) {
    uint32_t h = __torajs_arrprops_hash(arr_ptr);
    __torajs_arrprops_node_t **prev = &__torajs_arrprops_table[h];
    __torajs_arrprops_node_t *n = *prev;
    while (n) {
        if (n->arr_ptr == arr_ptr) {
            *prev = n->next;
            if (n->dynobj != NULL) {
                /* dynobj is owned 1:1 by this entry; drop's the
                 * walks-and-frees path. */
                __torajs_value_drop_heap(n->dynobj);
            }
            free(n);
            return;
        }
        prev = &n->next;
        n = n->next;
    }
}

/* T-10.d.i — Type::Any boxed-value runtime.
 *
 * Layout: 24 bytes — [hdr 8: refcount/type_tag=ANY_BOX/flags]
 *                    [tag: u64 @ offset 8]
 *                    [value: u64 @ offset 16]
 *
 * Created by `xs[i]` reads on Array<Any> — the slot's (tag, value)
 * pair is copied into a fresh box so the SSA layer can carry it as
 * a single pointer Operand. Heap-typed (ANY_HEAP) values get an
 * extra rc_inc on box creation so the boxed-value owns its child
 * independent of the source array's lifetime; box drop reverses it.
 *
 * Per-read box allocation is the trade-off vs holding the SSA-layer
 * pair-passing complexity. T-10.e or v0.5+ may inline use-site fast
 * paths for `console.log(xs[i])` to avoid the box allocation. */

/* Box layout offsets. Universal heap header is 8 bytes. Kept here
 * for the in-file `__torajs_any_payload_eq` / `_any_to_str` /
 * `_any_any_strict_eq` / `_any_strict_eq` / etc. that still read
 * box fields by const offset (P2.3-b/c/d follow-ups will move
 * those to Rust too). */
#define __TORAJS_ANY_BOX_TAG_OFF   8
#define __TORAJS_ANY_BOX_VAL_OFF   16
#define __TORAJS_ANY_BOX_SIZE      24

/* P2.3-a (2026-05-22 architecture-rewrite) — `__torajs_any_box`,
 * `__torajs_any_unbox_tag`, `__torajs_any_unbox_value`, and
 * `__torajs_any_payload_rc_inc` are now provided by the Rust
 * `torajs-anyvalue` crate (Layer-1 substrate; see
 * docs/architecture-rewrite.md). C definitions deleted in this
 * commit. The extern decls below let the rest of runtime_str.c
 * (any_to_str, strict_eq, etc.) keep calling them; the linker
 * resolves them from libtorajs_anyvalue.a at `tr build` time. */
extern void *__torajs_any_box(int64_t tag, int64_t value);
extern int64_t __torajs_any_unbox_tag(const void *box);
extern int64_t __torajs_any_unbox_value(const void *box);
extern void __torajs_any_payload_rc_inc(int64_t tag, int64_t val);

/* P4.2 Phase B+C — class prototype side table. Maps class runtime tag
 * (the same tag stored at OBJ_CLASS_TAG_OFF on instance allocation;
 * tag 0 = "no class") to the Any-box wrapping the class's
 * `__proto_<C>` dynobj. Populated once at module init via
 * `__torajs_proto_register` (emitted by synthesize_class_globals after
 * the per-class `__class_<C>` LetDecl); read by `__torajs_proto_get`
 * at `Object.getPrototypeOf(instance)` call sites. Static 256-entry
 * array is enough for the foreseeable class count; growing it
 * requires no protocol change.
 *
 * The table holds borrowed pointers to long-lived Any-boxes (the
 * `__proto_<C>` LetDecl's lifetime spans the program) — no rc bumps
 * here. `proto_get` returns the same pointer the let binding holds,
 * preserving identity across all readback sites. */
#define __TORAJS_MAX_CLASSES 256
static void *__torajs_protos_by_tag[__TORAJS_MAX_CLASSES];
/* P4.5 — parallel side table for `new.target`. `__class_<C>` Any-boxes
 * are registered here at module init. The synthesized `__new_<C>`
 * factory passes its own class object (looked up via class_get) into
 * the ctor as the hidden `__new_target` param. Static array keyed by
 * the same runtime class tag the obj_alloc site stamps onto each
 * instance. */
static void *__torajs_classes_by_tag[__TORAJS_MAX_CLASSES];

void __torajs_proto_register(int64_t tag, void *proto_anybox) {
    if (tag < 0 || tag >= __TORAJS_MAX_CLASSES) return;
    __torajs_protos_by_tag[tag] = proto_anybox;
}

void __torajs_class_register(int64_t tag, void *class_anybox) {
    if (tag < 0 || tag >= __TORAJS_MAX_CLASSES) return;
    __torajs_classes_by_tag[tag] = class_anybox;
}

/* Mirrors `proto_get` ownership contract: always returns an OWNED
 * Any-box (rc 1+). Registered class box gets rc_inc'd; absent/OOR
 * tag → fresh ANY_UNDEF box (spec §13.3.10 — `new.target` outside
 * a `new` call is undefined). */
void *__torajs_class_get(int64_t tag) {
    if (tag < 0 || tag >= __TORAJS_MAX_CLASSES) {
        return __torajs_any_box(__TORAJS_ANY_UNDEF, 0);
    }
    void *p = __torajs_classes_by_tag[tag];
    if (p == NULL) return __torajs_any_box(__TORAJS_ANY_UNDEF, 0);
    __torajs_rc_inc(p);
    return p;
}

/* Always returns an OWNED Any-box (rc 1). Callers don't rc_inc — the
 * box is theirs to drop. For the registered-class case, bumps the
 * stored `__proto_<C>` box's refcount; for the null/missing case,
 * allocates a fresh ANY_NULL box. Keeps caller refcount accounting
 * uniform across both paths. */
void *__torajs_proto_get(int64_t tag) {
    if (tag < 0 || tag >= __TORAJS_MAX_CLASSES) {
        return __torajs_any_box(__TORAJS_ANY_NULL, 0);
    }
    void *p = __torajs_protos_by_tag[tag];
    if (p == NULL) return __torajs_any_box(__TORAJS_ANY_NULL, 0);
    __torajs_rc_inc(p);
    return p;
}

/* P4.2 Phase B+C — `Object.getPrototypeOf(<any>)` on a dynobj-backed
 * Any value. Reads the `__proto__` field from the wrapped dynobj
 * and returns it as a fresh Any-box. Returns ANY_NULL when the box
 * doesn't wrap a dynobj, when the dynobj has no `__proto__` field
 * (root prototype), or for null / undefined / primitive Any tags
 * (spec §19.1.2.13 ToObject step throws for null/undefined, but
 * tora's subset returns NULL — pre-P7 Error type hierarchy lands).
 *
 * Identity preserved: the returned box wraps the SAME dynobj ptr
 * the parent prototype was stored at, so `Object.getPrototypeOf
 * (C.prototype) === B.prototype` holds by `any_payload_eq`'s ptr
 * compare on the underlying heap value. */
void *__torajs_get_proto_of_any(const void *box) {
    if (box == NULL) return __torajs_any_box(__TORAJS_ANY_NULL, 0);
    int64_t tag = *(const int64_t *)
        ((const uint8_t *)box + __TORAJS_ANY_BOX_TAG_OFF);
    if (tag != __TORAJS_ANY_HEAP) {
        return __torajs_any_box(__TORAJS_ANY_NULL, 0);
    }
    void *dynobj = (void *)(uintptr_t)*(const int64_t *)
        ((const uint8_t *)box + __TORAJS_ANY_BOX_VAL_OFF);
    if (dynobj == NULL) return __torajs_any_box(__TORAJS_ANY_NULL, 0);
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)dynobj;
    if (h->type_tag != __TORAJS_TAG_DYNOBJ) {
        return __torajs_any_box(__TORAJS_ANY_NULL, 0);
    }

    /* Build a transient "__proto__" key str (9 bytes). */
    static const char k_name[] = "__proto__";
    uint8_t *k = __torajs_str_alloc_pooled(9);
    memcpy(__TORAJS_STR_DATA(k), k_name, 9);

    if (!__torajs_dynobj_has(dynobj, k)) {
        __torajs_str_drop(k);
        return __torajs_any_box(__TORAJS_ANY_NULL, 0);
    }
    int64_t v_tag = (int64_t)__torajs_dynobj_get_tag(dynobj, k);
    int64_t v_val = (int64_t)__torajs_dynobj_get_value(dynobj, k);
    __torajs_str_drop(k);

    return __torajs_any_box(v_tag, v_val);
}

int64_t __torajs_str_eq(const uint8_t *a, const uint8_t *b);

/* P2.3-b (2026-05-22 architecture-rewrite) —
 * `__torajs_any_payload_eq` (static, in-file only) +
 * `__torajs_any_any_strict_eq` (Any === Any) +
 * `__torajs_any_strict_eq` (Any === concrete) are all provided
 * by the Rust `torajs-anyvalue` crate. Definitions deleted from
 * this file; the extern decls below let the in-file callers
 * (any_to_str / any_compare / any_arith / any_add / etc., all
 * still in C pre-P2.3-c/d) keep resolving the public symbols at
 * link time. The static payload_eq had no out-of-file callers
 * so it's just gone — its 2 in-file callers (any_any_strict_eq
 * + any_strict_eq) now live in Rust and use the Rust
 * payload_eq mirror. */
extern bool __torajs_any_any_strict_eq(const void *l, const void *r);
extern bool __torajs_any_strict_eq(const void *box, int64_t rhs_tag, int64_t rhs_value);

/* P2.3-d.1 (2026-05-23 architecture-rewrite) — `__torajs_any_to_number`
 * (public, called by ssa_lower's coerce_any_to_number) and
 * `__torajs_any_to_number_inner` (packed-pair, still used by the in-
 * file `__torajs_any_compare` / `__torajs_any_arith` until P2.3-d.2
 * and .3 port them) are now provided by the Rust `torajs-anyvalue`
 * crate. Both C definitions deleted in this commit; the extern decls
 * below let the in-file C callers keep resolving the public symbols
 * at link time. */
extern double __torajs_any_to_number(const void *box);
extern double __torajs_any_to_number_inner(int64_t tag, int64_t value);

/* P2.3-d.2 (2026-05-23 architecture-rewrite) — `__torajs_any_compare`
 * (relational ordering `<` / `<=` / `>` / `>=` per ES §7.2.13 +
 * §13.10) now provided by the Rust `torajs-anyvalue` crate.
 * Definition deleted from this file; the extern decl below lets
 * ssa_lower-emitted IR keep resolving the public symbol at link
 * time (no in-file C callers — any_compare is purely an ssa_lower
 * intrinsic, no internal chain via _inner-style helpers). */
extern bool __torajs_any_compare(int64_t op, int64_t lt, int64_t lv,
                                 int64_t rt, int64_t rv);

/* P2.3-d.3 (2026-05-23 architecture-rewrite) — `__torajs_any_arith`
 * (`-`, `*`, `/`, `%` per ES §13.6–§13.9) now provided by the Rust
 * `torajs-anyvalue` crate. Definition deleted from this file; the
 * extern decl below lets ssa_lower-emitted IR keep resolving the
 * public symbol at link time (no in-file C callers — pure ssa_lower
 * intrinsic, same as any_compare). */
extern void *__torajs_any_arith(int64_t op, int64_t lt, int64_t lv,
                                int64_t rt, int64_t rv);

/* P2.3-d.4 (2026-05-23 architecture-rewrite) — `__torajs_any_add`
 * (`+` per ES §13.15.3 ApplyStringOrNumericBinaryOperator) now
 * provided by the Rust `torajs-anyvalue` crate. Definition deleted
 * from this file; the extern decl below lets ssa_lower-emitted IR
 * keep resolving the public symbol at link time. The Rust impl
 * calls back into C-side `__torajs_str_concat` and
 * `__torajs_str_drop` for the Str-concatenation path (Layer-2
 * `torajs-str` rewrite ports those). With any_add ported, P2 —
 * Layer 1's any-value family is fully Rust. */
extern void *__torajs_any_add(int64_t lt, int64_t lv, int64_t rt,
                              int64_t rv);

void *__torajs_str_concat(const uint8_t *a, const uint8_t *b);
void *__torajs_i64_to_str(int64_t n);
void *__torajs_f64_to_str(double n);
void *__torajs_bool_to_str(int b);
void *__torajs_null_to_str(void);
void *__torajs_undefined_to_str(void);
double __torajs_str_to_number(const void *p);

/* P2.3-c — `__torajs_any_to_str` (Any → Str coercion per JS spec
 * §7.1.17) moved to the Rust `torajs-anyvalue` crate. The
 * placeholder "[object]" path stays bit-identical until P3 lands
 * per-type pretty-print. */
extern void *__torajs_any_to_str(int64_t tag, int64_t value);


/* P0.4 — ToBoolean(Any) per JS spec §7.1.2. Reads the box's tag
 * and payload, returns the spec-mandated boolean:
 *   ANY_NULL              → false
 *   ANY_BOOL              → value (i64 0 or 1)
 *   ANY_I64               → value != 0
 *   ANY_F64               → value != 0.0 AND not NaN  (IEEE Une on +0)
 *   ANY_HEAP / Str        → length > 0
 *   ANY_HEAP / other      → true (objects are always truthy)
 *   NULL box              → false
 */
bool __torajs_any_to_bool(const void *box) {
    if (box == NULL) return false;
    int64_t tag = *(const int64_t *)((const uint8_t *)box + __TORAJS_ANY_BOX_TAG_OFF);
    int64_t value = *(const int64_t *)((const uint8_t *)box + __TORAJS_ANY_BOX_VAL_OFF);
    switch (tag) {
        case __TORAJS_ANY_NULL: return false;
        /* P1.5 — ToBoolean(undefined) === false per spec §7.1.2 step 1.
         * Same answer as null but distinct origin (preserved by tag). */
        case __TORAJS_ANY_UNDEF: return false;
        case __TORAJS_ANY_BOOL: return value != 0;
        case __TORAJS_ANY_I64:  return value != 0;
        case __TORAJS_ANY_F64: {
            union { int64_t i; double d; } u = { .i = value };
            /* IEEE: NaN compares unequal to itself, +0 / -0 both
             * compare equal to 0.0 — so `d != 0.0` correctly maps
             * NaN → false (not satisfying "not equal" with itself
             * when also testing isnan...) actually `d != 0.0`
             * returns true for NaN. Use the canonical "ordered
             * not-equal" idiom instead: u.d == u.d (false for NaN)
             * AND u.d != 0.0. */
            return (u.d == u.d) && (u.d != 0.0);
        }
        case __TORAJS_ANY_HEAP: {
            void *child = (void *)(uintptr_t)value;
            if (child == NULL) return false;
            const __torajs_heap_header_t *h = (const __torajs_heap_header_t *)child;
            if (h->type_tag == __TORAJS_TAG_STR) {
                return __TORAJS_STR_LEN(child) > 0;
            }
            return true;
        }
        default: return false;
    }
}

/* P0.2 — `typeof <Any>` runtime dispatch per JS spec §13.5.3.
 * Reads the box's tag (and for ANY_HEAP, the inner heap header's
 * type_tag) and returns a fresh String holding the spec-mandated
 * literal: "number" / "string" / "boolean" / "object" / "function"
 * / "symbol" / "bigint" / "undefined". Tora has no real undefined
 * yet (P1) so ANY_NULL collapses to "object" — same as typeof null.
 * Strings are allocated via str_alloc_pooled so the result is a
 * regular owned Str the caller's drop walk handles.
 */
void *__torajs_any_typeof(const void *box) {
    const char *s = "object";
    size_t len = 6;
    if (box == NULL) {
        /* Bare null pointer (uninit / explicit cast) — treat as
         * "object" per spec (typeof null === "object"). */
    } else {
        int64_t tag = *(const int64_t *)((const uint8_t *)box + __TORAJS_ANY_BOX_TAG_OFF);
        switch (tag) {
            case __TORAJS_ANY_NULL: s = "object"; len = 6; break;
            /* P1.5 — typeof undefined === "undefined" per ES spec
             * §13.5.3 / §6.1.1.1. The ANY_UNDEF tag is the
             * substrate that lets us distinguish from ANY_NULL
             * (which keeps "object" per spec). */
            case __TORAJS_ANY_UNDEF: s = "undefined"; len = 9; break;
            case __TORAJS_ANY_BOOL: s = "boolean"; len = 7; break;
            case __TORAJS_ANY_I64:
            case __TORAJS_ANY_F64: s = "number"; len = 6; break;
            case __TORAJS_ANY_HEAP: {
                void *child = (void *)(uintptr_t)*(int64_t *)
                    ((const uint8_t *)box + __TORAJS_ANY_BOX_VAL_OFF);
                if (child == NULL) {
                    s = "object"; len = 6;
                } else {
                    const __torajs_heap_header_t *h =
                        (const __torajs_heap_header_t *)child;
                    switch (h->type_tag) {
                        case __TORAJS_TAG_STR: s = "string"; len = 6; break;
                        case __TORAJS_TAG_CLOSURE: s = "function"; len = 8; break;
                        case __TORAJS_TAG_SYMBOL: s = "symbol"; len = 6; break;
                        case __TORAJS_TAG_BIGINT: s = "bigint"; len = 6; break;
                        /* OBJ / ARR / REGEX / DATE / RESPONSE / WEAKREF /
                         * WEAKMAP / WEAKSET / ANY_BOX (nested) → "object" */
                        default: s = "object"; len = 6; break;
                    }
                }
                break;
            }
            default: s = "object"; len = 6; break;
        }
    }
    uint8_t *p = __torajs_str_alloc_pooled((uint64_t)len);
    memcpy((uint8_t *)p + __TORAJS_STR_HDR_SIZE, s, len);
    return p;
}

/* P2.3-a — `__torajs_any_box_drop` is now provided by the Rust
 * `torajs-anyvalue` crate. The Rust version is byte-equivalent
 * (null check, STATIC_LITERAL bypass, rc_dec hit-zero gate,
 * ANY_HEAP child value_drop_heap recurse, dealloc 24 bytes via
 * `std::alloc::dealloc` matching the same allocator the Rust
 * side used for the matching `__torajs_any_box` allocation). */
extern void __torajs_any_box_drop(void *box);

/* T-10.d.i — `console.log(any_value)` dispatch. Reads the box's tag
 * and routes to the matching primitive printer. ANY_HEAP recurses
 * through the heap value's universal type_tag for Str (the only
 * pretty-printable heap type covered by T-10.d.i; Obj / Arr / Date
 * etc. land later — for now those print as a placeholder). Output
 * matches bun's `console.log` formatting for primitives:
 *   1        → "1"
 *   1.5      → "1.5"
 *   'hello'  → "hello"  (no surrounding quotes for top-level)
 *   true     → "true"
 *   null     → "null"
 *   undefined → tr maps to null → "null"
 *
 * Trailing newline matches the existing print_* helpers; multi-arg
 * console.log goes through ssa_lower's space-joiner which calls
 * this for each arg in turn (T-10.d adds the multi-arg variant
 * `__torajs_print_any_no_newline`). For T-10.d.i the single-arg
 * form is enough to validate the round-trip end-to-end. */
extern void print_i64(int64_t);
extern void print_f64(double);
extern void print_bool(_Bool);
extern void __torajs_str_print(const uint8_t *);

void __torajs_print_any(const void *box) {
    if (box == NULL) {
        fputs("null\n", stdout);
        return;
    }
    int64_t tag = *(const int64_t *)((const uint8_t *)box + __TORAJS_ANY_BOX_TAG_OFF);
    int64_t v = *(const int64_t *)((const uint8_t *)box + __TORAJS_ANY_BOX_VAL_OFF);
    switch (tag) {
        case __TORAJS_ANY_NULL: fputs("null\n", stdout); break;
        /* P1.5 — console.log(undefined) → "undefined". Bun output. */
        case __TORAJS_ANY_UNDEF: fputs("undefined\n", stdout); break;
        case __TORAJS_ANY_BOOL: print_bool((_Bool)(v != 0)); break;
        case __TORAJS_ANY_I64:  print_i64(v); break;
        case __TORAJS_ANY_F64: {
            double d;
            memcpy(&d, &v, sizeof(double));
            print_f64(d);
            break;
        }
        case __TORAJS_ANY_HEAP: {
            void *child = (void *)(uintptr_t)v;
            if (child == NULL) {
                fputs("null\n", stdout);
                break;
            }
            __torajs_heap_header_t *h = (__torajs_heap_header_t *)child;
            if (h->type_tag == __TORAJS_TAG_STR) {
                __torajs_str_print((const uint8_t *)child);
            } else {
                /* Obj / Arr / Closure / RegExp / Date pretty-print
                 * lands with T-10.e. For now print a placeholder so
                 * the user sees something rather than silent / crash. */
                fputs("[object]\n", stdout);
            }
            break;
        }
        default:
            fputs("[unknown-any-tag]\n", stdout);
            break;
    }
}

/* __torajs_substr_create + __torajs_substr_drop moved to the
 * `torajs-str::substr` Rust module (P3.1-b, 2026-05-23). Forward
 * decls live near the layout-constants block at the top of this
 * file. The drop semantics — INLINE flag check, parent rc-dec via
 * __torajs_str_drop, own-rc dec, pool push or libc free — are
 * mirrored byte-for-byte in `substr::SubstrBlock::drop_pool_aware`.
 */

/* Substr → i64: byte at position `i` (zero-extended). Out-of-bounds
 * returns 0, matching `__torajs_str_char_code_at` semantics for OWNED
 * Str. Hot path on RPN-style demos that iterate `tok.charCodeAt(i)`
 * over view substrings from `expr.split(" ")` — explicit always_inline
 * so LTO collapses the per-call dispatch to a 3-load + zext sequence
 * inside the caller's loop body. */
__attribute__((always_inline))
int64_t __torajs_substr_char_code_at(const uint8_t *v, int64_t i) {
    uint64_t len = __TORAJS_SUBSTR_LEN(v);
    if (i < 0 || (uint64_t)i >= len) return 0;
    const uint8_t *parent = *(const uint8_t *const *)(v + __TORAJS_SUBSTR_PARENT_OFF);
    uint64_t offset = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    return (int64_t)parent[__TORAJS_STR_HDR_SIZE + offset + (uint64_t)i];
}

/* Bytewise compare a Substr against an OWNED Str. Used by switch /
 * `===` dispatch when the rhs is a known short literal (ssa_lower
 * may further inline the byte chain). Returns 1 iff lengths equal
 * AND bytes equal. */
int64_t __torajs_substr_eq_str(const uint8_t *v, const uint8_t *s) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t s_len = __TORAJS_STR_LEN(s);
    if (v_len != s_len) return 0;
    if (v_len == 0) return 1;
    const uint8_t *parent = *(const uint8_t *const *)(v + __TORAJS_SUBSTR_PARENT_OFF);
    uint64_t offset = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    const uint8_t *v_data = parent + __TORAJS_STR_HDR_SIZE + offset;
    return memcmp(v_data, __TORAJS_STR_CDATA(s), (size_t)v_len) == 0 ? 1 : 0;
}

/* Materialize a Substr into a fresh OWNED Str (for crossing fn-call
 * boundaries that expect Type::Str — Phase Substr.B; Phase Substr.C
 * will mono-morphize the callee to accept Substr directly and avoid
 * this allocation entirely). */
void *__torajs_substr_to_owned(const uint8_t *v) {
    uint64_t len = __TORAJS_SUBSTR_LEN(v);
    const uint8_t *parent = *(const uint8_t *const *)(v + __TORAJS_SUBSTR_PARENT_OFF);
    uint64_t offset = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    uint8_t *p = __torajs_str_alloc_pooled(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), parent + __TORAJS_STR_HDR_SIZE + offset, (size_t)len);
    return p;
}

/* Helper: resolve a Substr's data base (parent.bytes + offset). */
static inline const uint8_t *substr_data_(const uint8_t *v) {
    const uint8_t *parent = *(const uint8_t *const *)(v + __TORAJS_SUBSTR_PARENT_OFF);
    uint64_t offset = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    return parent + __TORAJS_STR_HDR_SIZE + offset;
}

/* View-aware concat: one alloc + two memcpys, no intermediate
 * materialize. Phase B's straightforward `substr + s` path goes
 * through substr_to_owned + str_concat (2 allocs, 3 memcpys); the
 * helpers below collapse that to (1 alloc, 2 memcpys) — same cost
 * as a plain Str + Str concat. */
void *__torajs_substr_concat_substr_str(const uint8_t *v, const uint8_t *s) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint8_t *p = __torajs_str_alloc_pooled(v_len + s_len);
    uint8_t *out = __TORAJS_STR_DATA(p);
    if (v_len) memcpy(out, substr_data_(v), (size_t)v_len);
    if (s_len) memcpy(out + v_len, __TORAJS_STR_CDATA(s), (size_t)s_len);
    return p;
}

void *__torajs_substr_concat_str_substr(const uint8_t *s, const uint8_t *v) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint8_t *p = __torajs_str_alloc_pooled(s_len + v_len);
    uint8_t *out = __TORAJS_STR_DATA(p);
    if (s_len) memcpy(out, __TORAJS_STR_CDATA(s), (size_t)s_len);
    if (v_len) memcpy(out + s_len, substr_data_(v), (size_t)v_len);
    return p;
}

void *__torajs_substr_concat_substr_substr(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = __TORAJS_SUBSTR_LEN(a);
    uint64_t b_len = __TORAJS_SUBSTR_LEN(b);
    uint8_t *p = __torajs_str_alloc_pooled(a_len + b_len);
    uint8_t *out = __TORAJS_STR_DATA(p);
    if (a_len) memcpy(out, substr_data_(a), (size_t)a_len);
    if (b_len) memcpy(out + a_len, substr_data_(b), (size_t)b_len);
    return p;
}

/* Substr.startsWith / endsWith / includes / indexOf — view-aware
 * variants that read bytes from parent + offset without materializing.
 * Needle is a Str (the common case from string literals). */
int8_t __torajs_substr_starts_with(const uint8_t *v, const uint8_t *n) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t n_len = __TORAJS_STR_LEN(n);
    if (n_len > v_len) return 0;
    if (n_len == 0) return 1;
    return memcmp(substr_data_(v), __TORAJS_STR_CDATA(n), (size_t)n_len) == 0 ? 1 : 0;
}

int8_t __torajs_substr_ends_with(const uint8_t *v, const uint8_t *n) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t n_len = __TORAJS_STR_LEN(n);
    if (n_len > v_len) return 0;
    if (n_len == 0) return 1;
    return memcmp(substr_data_(v) + (v_len - n_len), __TORAJS_STR_CDATA(n), (size_t)n_len) == 0 ? 1 : 0;
}

int8_t __torajs_substr_includes(const uint8_t *v, const uint8_t *n) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t n_len = __TORAJS_STR_LEN(n);
    if (n_len == 0) return 1;
    if (n_len > v_len) return 0;
    const uint8_t *v_data = substr_data_(v);
    const uint8_t *n_data = __TORAJS_STR_CDATA(n);
    uint64_t end = v_len - n_len;
    for (uint64_t i = 0; i <= end; i++) {
        if (memcmp(v_data + i, n_data, (size_t)n_len) == 0) return 1;
    }
    return 0;
}

int64_t __torajs_substr_index_of(const uint8_t *v, const uint8_t *n) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t n_len = __TORAJS_STR_LEN(n);
    if (n_len == 0) return 0;
    if (n_len > v_len) return -1;
    const uint8_t *v_data = substr_data_(v);
    const uint8_t *n_data = __TORAJS_STR_CDATA(n);
    uint64_t end = v_len - n_len;
    for (uint64_t i = 0; i <= end; i++) {
        if (memcmp(v_data + i, n_data, (size_t)n_len) == 0) return (int64_t)i;
    }
    return -1;
}

/* Substr.slice / substring — view-of-view. The new Substr's parent is
 * the SAME root parent (drop chain stays depth-1). Standalone (not
 * INLINE) — its 32-byte struct is a separate malloc and substr_drop
 * will free it. Negative index handling (slice wraps, substring
 * clamps + swaps) matches the corresponding str helpers. */
/* V3-18 m1.h.37 — `s.charAt(i)` on an OWNED Str. Per JS spec
 * §21.1.3.1: out-of-range i (negative OR >= len) returns "" (not
 * the same as charCodeAt's NaN — charAt's empty-string is a true
 * spec choice). Returns a length-1 Substr view on the receiver
 * for in-range i, or a length-0 Substr (offset clamped to 0) for
 * OOB. Pre-fix tora's charAt lower called substr_create directly
 * which trusted the caller's idx, so charAt(-1) printed garbage
 * bytes via the parent-pointer math. */
void *__torajs_str_char_at(void *s, int64_t i) {
    if (s == NULL) {
        return __torajs_substr_create(s, 0, 0);
    }
    uint64_t len = __TORAJS_STR_LEN(s);
    if (i < 0 || (uint64_t)i >= len) {
        return __torajs_substr_create(s, 0, 0);
    }
    return __torajs_substr_create(s, (uint64_t)i, 1);
}

void *__torajs_substr_slice(const uint8_t *v, int64_t start, int64_t end) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    int64_t s = start < 0 ? (int64_t)v_len + start : start;
    int64_t e = end < 0 ? (int64_t)v_len + end : end;
    if (s < 0) s = 0;
    if (e < 0) e = 0;
    if (s > (int64_t)v_len) s = (int64_t)v_len;
    if (e > (int64_t)v_len) e = (int64_t)v_len;
    if (s > e) s = e;
    void *parent = *(uint8_t **)(v + __TORAJS_SUBSTR_PARENT_OFF);
    uint64_t v_off = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    return __torajs_substr_create(parent, v_off + (uint64_t)s, (uint64_t)(e - s));
}

/* Whitespace bytes (TS String.prototype.trim spec) — keep in sync with
 * the str_trim implementation. ASCII subset is enough for tr's byte-Str
 * model; full unicode whitespace will need utf-8-aware code later. */
static inline int substr_is_ws_(uint8_t b) {
    return b == ' ' || b == '\t' || b == '\n' || b == '\r' || b == '\v' || b == '\f';
}

/* Substr.trim — return a Substr whose offset/len are narrowed past the
 * leading and trailing whitespace bytes. Same root parent (drop chain
 * stays depth-1). 32-byte malloc, no byte copy. */
void *__torajs_substr_trim(const uint8_t *v) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t v_off = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    void *parent = *(uint8_t **)(v + __TORAJS_SUBSTR_PARENT_OFF);
    const uint8_t *base = (const uint8_t *)parent + __TORAJS_STR_HDR_SIZE + v_off;
    uint64_t lo = 0;
    while (lo < v_len && substr_is_ws_(base[lo])) lo++;
    uint64_t hi = v_len;
    while (hi > lo && substr_is_ws_(base[hi - 1])) hi--;
    return __torajs_substr_create(parent, v_off + lo, hi - lo);
}

void *__torajs_substr_trim_start(const uint8_t *v) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t v_off = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    void *parent = *(uint8_t **)(v + __TORAJS_SUBSTR_PARENT_OFF);
    const uint8_t *base = (const uint8_t *)parent + __TORAJS_STR_HDR_SIZE + v_off;
    uint64_t lo = 0;
    while (lo < v_len && substr_is_ws_(base[lo])) lo++;
    return __torajs_substr_create(parent, v_off + lo, v_len - lo);
}

void *__torajs_substr_trim_end(const uint8_t *v) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint64_t v_off = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    void *parent = *(uint8_t **)(v + __TORAJS_SUBSTR_PARENT_OFF);
    const uint8_t *base = (const uint8_t *)parent + __TORAJS_STR_HDR_SIZE + v_off;
    uint64_t hi = v_len;
    while (hi > 0 && substr_is_ws_(base[hi - 1])) hi--;
    return __torajs_substr_create(parent, v_off, hi);
}

void *__torajs_substr_substring(const uint8_t *v, int64_t start, int64_t end) {
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    if (start < 0) start = 0;
    if (end < 0) end = 0;
    if (start > (int64_t)v_len) start = (int64_t)v_len;
    if (end > (int64_t)v_len) end = (int64_t)v_len;
    if (start > end) {
        int64_t tmp = start;
        start = end;
        end = tmp;
    }
    void *parent = *(uint8_t **)(v + __TORAJS_SUBSTR_PARENT_OFF);
    uint64_t v_off = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
    return __torajs_substr_create(parent, v_off + (uint64_t)start, (uint64_t)(end - start));
}

/* ============================================================
 * Arr layout (Phase 2A — universal heap header)
 *
 * Arr = [header:8][len:8][cap:8][slots:N*8]
 * refcount + type_tag + flags at offset 0   (universal header)
 * len  at offset 8
 * cap  at offset 16
 * slots at offset 24
 *
 * Sharing: ssa_lower emits __torajs_rc_inc at every alias-introducing
 * site (let arr2 = arr / arr.slice / spread / ...). __torajs_arr_drop
 * is refcount-aware (dec; free at 0). Element-walk drop fires at
 * Type::Arr drop site for refcounted element types.
 * ============================================================ */

/* T-13.5 (v0.4.0) — Array deque substrate. cap shrinks from u64 to
 * u32 (max ~4 billion elems still way past anything realistic);
 * the freed 4 bytes hold `head_offset` u32, the front-shift counter.
 * `slot[i]` now lives at `data + (head + i) * 8` so `arr.shift()`
 * is O(1) (just bump head) instead of O(n) memmove. Compact-when-
 * full (push hits cap with head > 0) reclaims the leading slack
 * via one memmove of the live range; grow falls back when no
 * slack exists. Same HDR_SIZE = 24 means no .o-layout change for
 * existing inkwell IR sites that hardcode the data offset. */
#define __TORAJS_ARR_HDR_SIZE   24
#define __TORAJS_ARR_LEN(p)     (*(uint64_t *)((const uint8_t *)(p) + 8))
#define __TORAJS_ARR_CAP(p)     (*(uint32_t *)((const uint8_t *)(p) + 16))
#define __TORAJS_ARR_HEAD(p)    (*(uint32_t *)((const uint8_t *)(p) + 20))
#define __TORAJS_ARR_DATA(p)    ((uint8_t *)(p) + __TORAJS_ARR_HDR_SIZE)
#define __TORAJS_ARR_CDATA(p)   ((const uint8_t *)(p) + __TORAJS_ARR_HDR_SIZE)
/* Head-aware slot macros — `i` is the user-visible index, the
 * physical slot is `head + i`. Byte offset = HDR_SIZE + (head+i)*8. */
#define __TORAJS_ARR_SLOT(p, i) \
    (__TORAJS_ARR_DATA(p) + ((uint64_t)__TORAJS_ARR_HEAD(p) + (uint64_t)(i)) * 8)
#define __TORAJS_ARR_CSLOT(p, i) \
    (__TORAJS_ARR_CDATA(p) + ((uint64_t)__TORAJS_ARR_HEAD(p) + (uint64_t)(i)) * 8)
/* Raw-physical-slot helpers — bypass head_offset, used by arr_free /
 * compact / fresh-alloc paths that operate on the underlying byte
 * layout directly. */
#define __TORAJS_ARR_DATA_RAW_SLOT(p, n) \
    (__TORAJS_ARR_DATA(p) + (uint64_t)(n) * 8)

/* Append every element of `src` to `dst` via a single memcpy. Caller
 * MUST have pre-sized dst's cap to fit (typical: array literal with
 * spreads pre-computes total length and allocs once). Bumps dst's
 * len. Both arrays are the same 8-byte-slot layout — element type
 * doesn't matter at this layer. */
void __torajs_arr_extend_unchecked(uint8_t *dst, const uint8_t *src) {
    uint64_t dst_len = __TORAJS_ARR_LEN(dst);
    uint64_t src_len = __TORAJS_ARR_LEN(src);
    if (src_len == 0) return;
    /* Both source and dst can have head_offset > 0 — the head-aware
     * SLOT/CSLOT macros handle the offset transparently. (T-13.5.) */
    memcpy(__TORAJS_ARR_SLOT(dst, dst_len),
           __TORAJS_ARR_CSLOT(src, 0),
           (size_t)src_len * 8);
    __TORAJS_ARR_LEN(dst) = dst_len + src_len;
}

/* `Math.sign(x)` — JS spec: +1 / -1 / preserve-zero. NaN handling
 * elided (subset doesn't expose NaN). libc has no `sign`, so this
 * lives here rather than in the inkwell-side `define_math_unary`. */
double __torajs_math_sign(double x) {
    if (x > 0.0) return 1.0;
    if (x < 0.0) return -1.0;
    return x;  /* preserves -0.0 / +0.0 per JS spec */
}

/* `Math.round(x)` — JS rounds half-values toward +∞:
 *   round(2.5)  === 3   (libc agrees)
 *   round(-2.5) === -2  (libc disagrees: returns -3)
 *   round(2.4)  === 2
 * The simple `floor(x + 0.5)` form matches JS spec; we route here
 * instead of libc round because libc rounds away from zero. */
double __torajs_math_floor(double);  /* fwd-decl from inkwell side */
double __torajs_math_round(double x) {
    /* floor is defined in the inkwell-emitted module; libc fallback
     * works too because the linker resolves either way. */
    return __torajs_math_floor(x + 0.5);
}

/* `s.repeat(n)` — fresh String containing `s` concatenated n times.
 * Single malloc + n memcpy's. n<=0 returns the empty string. */
void *__torajs_str_repeat(const uint8_t *s, int64_t n) {
    if (n < 0) n = 0;
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint64_t out_len = s_len * (uint64_t)n;
    uint8_t *p = __torajs_str_alloc_pooled(out_len);
    if (s_len == 0 || n == 0) return p;
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    for (int64_t i = 0; i < n; i++) {
        memcpy(p_data + (size_t)i * (size_t)s_len, s_data, (size_t)s_len);
    }
    return p;
}

/* Internal helper — alloc a fresh Arr heap with refcount=1 + type_tag
 * set + len/cap written. Caller fills the slot data at
 * __TORAJS_ARR_DATA(p). Single-source-of-truth for every Arr alloc
 * in this file. */
static uint8_t *arr_alloc_(uint64_t len, uint64_t cap) {
    uint8_t *p = (uint8_t *)malloc(__TORAJS_ARR_HDR_SIZE + (size_t)cap * 8);
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_ARR;
    h->flags = 0;
    __TORAJS_ARR_LEN(p) = len;
    __TORAJS_ARR_CAP(p) = (uint32_t)cap;
    __TORAJS_ARR_HEAD(p) = 0;  /* T-13.5 — fresh alloc starts head=0 */
    return p;
}

/* `arr.slice(start, end)` — fresh array containing the [start, end)
 * range. Both indices are clamped to [0, arr.len]. Single malloc +
 * one memcpy. Element-type-agnostic (8-byte slots). */
void *__torajs_arr_slice(const uint8_t *arr, int64_t start, int64_t end) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    /* V3-18 m1.h.35 — JS spec §22.1.3.25 negative-index handling:
     *   relativeStart < 0 → max(len + relativeStart, 0)
     *   relativeStart >= 0 → min(relativeStart, len)
     * Same for end. Pre-fix `arr.slice(-2)` clamped to 0 instead
     * of `len - 2`, returning the whole array instead of the tail. */
    int64_t ilen = (int64_t)len;
    int64_t lo = start < 0
        ? (start + ilen < 0 ? 0 : start + ilen)
        : (start > ilen ? ilen : start);
    int64_t hi = end < 0
        ? (end + ilen < 0 ? 0 : end + ilen)
        : (end > ilen ? ilen : end);
    if (hi < lo) hi = lo;
    uint64_t out_len = (uint64_t)(hi - lo);
    uint8_t *p = arr_alloc_(out_len, out_len); /* cap = len; no extra slack */
    if (out_len > 0) {
        memcpy(__TORAJS_ARR_DATA(p), __TORAJS_ARR_CSLOT(arr, lo), (size_t)out_len * 8);
    }
    return p;
}

/* Format an i64 as a fresh String heap object. Used by `+` when one
 * operand is Number and the other String — JS coerces the number to
 * its decimal string form. snprintf gives enough buffer for any i64
 * (max 20 digits + sign + null = 22 bytes). */
void *__torajs_i64_to_str(int64_t n) {
    char buf[24];
    int written = snprintf(buf, sizeof(buf), "%lld", (long long)n);
    if (written < 0) written = 0;
    uint64_t len = (uint64_t)written;
    uint8_t *p = __torajs_str_alloc_pooled(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), buf, (size_t)len);
    return p;
}

/* Same shape for f64. Uses %g for short round-trip-friendly output —
 * matches JS's String(n) for the integer-valued cases we exercise.
 * (Full IEEE-754 round-trip requires more care; we'll punt on that
 * until a test demands it.) */
/* V3-18 m1.h.12 — `console.log(arr)` pretty-print. Bun shape:
 * `[ 1, 2, 3 ]` (note spaces). Empty: `[]`. Per-element format
 * lives in a per-type helper below; this is the I64 element
 * variant called when the receiver is statically Array<I64>. */
#define __TORAJS_ARR_DATA_OFF 24

void __torajs_arr_print_i64(void *arr) {
    if (arr == NULL) {
        fputs("undefined\n", stdout);
        return;
    }
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    if (len == 0) {
        fputs("[]\n", stdout);
        return;
    }
    fputs("[ ", stdout);
    uint32_t head = *(uint32_t *)((uint8_t *)arr + 20);
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0) fputs(", ", stdout);
        int64_t v = *(int64_t *)((uint8_t *)arr + __TORAJS_ARR_DATA_OFF + ((uint64_t)head + i) * 8);
        printf("%lld", (long long)v);
    }
    fputs(" ]\n", stdout);
}

void __torajs_arr_print_f64(void *arr) {
    if (arr == NULL) {
        fputs("undefined\n", stdout);
        return;
    }
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    if (len == 0) {
        fputs("[]\n", stdout);
        return;
    }
    fputs("[ ", stdout);
    uint32_t head = *(uint32_t *)((uint8_t *)arr + 20);
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0) fputs(", ", stdout);
        double v = *(double *)((uint8_t *)arr + __TORAJS_ARR_DATA_OFF + ((uint64_t)head + i) * 8);
        if (v != v) { fputs("NaN", stdout); }
        else if (v == 1.0/0.0) { fputs("Infinity", stdout); }
        else if (v == -1.0/0.0) { fputs("-Infinity", stdout); }
        else printf("%g", v);
    }
    fputs(" ]\n", stdout);
}

void __torajs_arr_print_bool(void *arr) {
    if (arr == NULL) {
        fputs("undefined\n", stdout);
        return;
    }
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    if (len == 0) {
        fputs("[]\n", stdout);
        return;
    }
    fputs("[ ", stdout);
    uint32_t head = *(uint32_t *)((uint8_t *)arr + 20);
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0) fputs(", ", stdout);
        int64_t v = *(int64_t *)((uint8_t *)arr + __TORAJS_ARR_DATA_OFF + ((uint64_t)head + i) * 8);
        fputs(v ? "true" : "false", stdout);
    }
    fputs(" ]\n", stdout);
}

void __torajs_arr_print_str(void *arr) {
    if (arr == NULL) {
        fputs("undefined\n", stdout);
        return;
    }
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    if (len == 0) {
        fputs("[]\n", stdout);
        return;
    }
    fputs("[ ", stdout);
    uint32_t head = *(uint32_t *)((uint8_t *)arr + 20);
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0) fputs(", ", stdout);
        void *s = *(void **)((uint8_t *)arr + __TORAJS_ARR_DATA_OFF + ((uint64_t)head + i) * 8);
        if (s == NULL) {
            fputs("\"\"", stdout);
            continue;
        }
        uint64_t slen = *(uint64_t *)((uint8_t *)s + 8);
        const uint8_t *sdata = (const uint8_t *)s + 16;
        fputc('"', stdout);
        fwrite(sdata, 1, (size_t)slen, stdout);
        fputc('"', stdout);
    }
    fputs(" ]\n", stdout);
}

/* V3-18 m1.h.34 — single-Substr printer (console.log(substr) path).
 * Substr layout is { hdr@0, len@8, parent@16, offset@24 } — different
 * from Str's inline-data-at-16 — so the existing str_print can't be
 * shared. NULL → "null\n" matching str_print's null-guard shape. */
void __torajs_substr_print(void *v) {
    if (v == NULL) {
        fputs("null\n", stdout);
        return;
    }
    uint64_t len = __TORAJS_SUBSTR_LEN(v);
    uint8_t *parent = __TORAJS_SUBSTR_PARENT(v);
    uint64_t offset = __TORAJS_SUBSTR_OFFSET(v);
    if (len > 0) {
        fwrite(parent + __TORAJS_STR_HDR_SIZE + offset, 1, (size_t)len, stdout);
    }
    fputc('\n', stdout);
}

/* V3-18 m1.h.28 — arr-of-Substr printer. Each slot points at a
 * Substr header whose layout is { hdr@0, len@8, parent@16, offset@24 }
 * — different from Str's inline-data-at-16. Without this dispatch
 * `console.log("a-b-c".split("-"))` printed garbage bytes from
 * the parent pointer interpreted as data. */
void __torajs_arr_print_substr(void *arr) {
    if (arr == NULL) {
        fputs("undefined\n", stdout);
        return;
    }
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    if (len == 0) {
        fputs("[]\n", stdout);
        return;
    }
    fputs("[ ", stdout);
    uint32_t head = *(uint32_t *)((uint8_t *)arr + 20);
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0) fputs(", ", stdout);
        void *v = *(void **)((uint8_t *)arr + __TORAJS_ARR_DATA_OFF + ((uint64_t)head + i) * 8);
        if (v == NULL) {
            fputs("\"\"", stdout);
            continue;
        }
        uint64_t slen = __TORAJS_SUBSTR_LEN(v);
        uint8_t *parent = __TORAJS_SUBSTR_PARENT(v);
        uint64_t offset = __TORAJS_SUBSTR_OFFSET(v);
        fputc('"', stdout);
        fwrite(parent + __TORAJS_STR_HDR_SIZE + offset, 1, (size_t)slen, stdout);
        fputc('"', stdout);
    }
    fputs(" ]\n", stdout);
}

/* V3-18 m1.h.9 — console.log of a f64 must format NaN as "NaN"
 * (capitalized) and Infinity / -Infinity per spec, matching the
 * String(d) shape. Replaces the IR-emitted printf("%g\n") path. */
/* Format `d` per ECMA-262 §6.1.6.1.13 / §22.1.3.6:
 *
 *   - If d is an integer-valued double in (-1e21, 1e21): print as
 *     decimal with no fractional part (e.g. 10 → "10", 2500 →
 *     "2500"), never exponential notation. printf("%g") with low
 *     precision gives "1e+01" / "2.5e+03" which JS spec forbids
 *     for the in-range integer case.
 *   - Otherwise: shortest decimal that roundtrips. Try-precisions
 *     loop over `%.*g` from 1 → 17. Slow vs. Ryu/Grisu but only
 *     the print path hits it; output is byte-equal to v8/JSC for
 *     every f64 value. (Future perf: drop in Ryu.)
 *
 * Returns the number of bytes written (excluding NUL), or -1 on
 * overflow. buf must be at least 32 bytes. */
static int torajs_f64_shortest(double d, char *buf, size_t cap) {
    /* Integer-valued in spec's plain-decimal range: §6.1.6.1.13
     * step 5: when 0 < n ≤ 21 (and k ≤ n) print as decimal. The
     * abs-bound 1e21 is the spec's threshold for switching to
     * exponential. */
    double abs_d = d < 0 ? -d : d;
    /* Integer-valued check via floor — safe across the full f64 range
     * (the (long long) cast is UB past i64::MAX, e.g. for 9.99e18). */
    if (d == floor(d) && abs_d < 1e21) {
        /* %.0f gives the plain integer form (no exponent) for any
         * integer-valued double in spec range, including big ones
         * like 9.99e18 → "9989999999999999488" (the actual f64
         * representation, matching v8/JSC). ±0 sign handling is the
         * caller's responsibility — console.log(-0) shows "-0",
         * String(-0) returns "0" (ECMA-262 §22.1.3.6). */
        return snprintf(buf, cap, "%.0f", d);
    }
    for (int prec = 1; prec <= 17; prec++) {
        int written = snprintf(buf, cap, "%.*g", prec, d);
        if (written < 0 || (size_t)written >= cap) return -1;
        double parsed = strtod(buf, NULL);
        if (parsed == d) return written;
    }
    return snprintf(buf, cap, "%.17g", d);
}

void __torajs_print_f64_js(double d) {
    if (d != d) {
        fputs("NaN\n", stdout);
        return;
    }
    if (d == 1.0 / 0.0) {
        fputs("Infinity\n", stdout);
        return;
    }
    if (d == -1.0 / 0.0) {
        fputs("-Infinity\n", stdout);
        return;
    }
    char buf[32];
    int n = torajs_f64_shortest(d, buf, sizeof(buf));
    if (n < 0) n = 0;
    fwrite(buf, 1, (size_t)n, stdout);
    fputc('\n', stdout);
}

void *__torajs_f64_to_str(double d) {
    /* V3-18 m1.h.9 — JS spec §22.1.3.6 String(NaN) returns "NaN"
     * (capitalized); strtod / printf use the lowercase "nan" by
     * default. Same for "Infinity" / "-Infinity". Special-case
     * before snprintf. */
    if (d != d) {
        uint8_t *p = __torajs_str_alloc_pooled(3);
        memcpy(__TORAJS_STR_DATA(p), "NaN", 3);
        return p;
    }
    if (d == 1.0 / 0.0) {
        uint8_t *p = __torajs_str_alloc_pooled(8);
        memcpy(__TORAJS_STR_DATA(p), "Infinity", 8);
        return p;
    }
    if (d == -1.0 / 0.0) {
        uint8_t *p = __torajs_str_alloc_pooled(9);
        memcpy(__TORAJS_STR_DATA(p), "-Infinity", 9);
        return p;
    }
    char buf[32];
    int written = torajs_f64_shortest(d, buf, sizeof(buf));
    if (written < 0) written = 0;
    /* §22.1.3.6: String(-0) returns "0" (no sign). console.log(-0)
     * keeps the sign via node's util.inspect — that path hits
     * __torajs_print_f64_js, not here. */
    int off = 0;
    if (d == 0.0 && written >= 1 && buf[0] == '-') {
        off = 1;
        written -= 1;
    }
    uint64_t len = (uint64_t)written;
    uint8_t *p = __torajs_str_alloc_pooled(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), buf + off, (size_t)len);
    return p;
}

/* __torajs_str_to_number moved to torajs-str::to_number (P3.1-c,
 * 2026-05-23). The Rust impl uses f64::from_str (textbook fast-
 * path, identical accuracy guarantees to libc strtod) and the same
 * trim / Infinity / NaN literal handling. Forward decl at line
 * 1794 still pins the prototype other fns in this TU reference. */

/* V3-18 m1.d — JS spec §7.1.17 ToString:
 *   Boolean true  → "true"
 *   Boolean false → "false"
 *   null          → "null"
 *   undefined     → "undefined" (deferred until undefined ships)
 * Returns a fresh heap Str — caller drops normally. */
void *__torajs_bool_to_str(int b) {
    const char *s = b ? "true" : "false";
    uint64_t len = b ? 4 : 5;
    uint8_t *p = __torajs_str_alloc_pooled(len);
    memcpy(__TORAJS_STR_DATA(p), s, (size_t)len);
    return p;
}

void *__torajs_null_to_str(void) {
    const char *s = "null";
    uint8_t *p = __torajs_str_alloc_pooled(4);
    memcpy(__TORAJS_STR_DATA(p), s, 4);
    return p;
}

/* P1.5 — `String(undefined)` / `${undefined}` produce "undefined"
 * per ES spec §6.1.1. Mirror of __torajs_null_to_str for the
 * ANY_UNDEF tag dispatched in __torajs_any_to_str. */
void *__torajs_undefined_to_str(void) {
    const char *s = "undefined";
    uint8_t *p = __torajs_str_alloc_pooled(9);
    memcpy(__TORAJS_STR_DATA(p), s, 9);
    return p;
}


/* __torajs_str_eq moved to torajs-str::eq (P3.1-c, 2026-05-23).
 * Forward decl at line 1729 still pins the prototype for callers
 * in this TU. The Rust core (`eq::bytes_eq`) is a pure-Rust slice
 * comparison; the FFI wrapper reads Str layout via STR_LEN_OFF /
 * STR_DATA_OFF constants that mirror this file's macros. */

/* v0.2 #3 — Object.is(a, b) for Type::Number arguments. ECMA-262
 * §7.2.10 SameValue: behaves like `===` except (i) NaN is the same
 * value as NaN, and (ii) +0 and -0 are different values. The ±0
 * check is bit-level since IEEE 754 says 0.0 == -0.0 evaluates true
 * under FCmp. */
int64_t __torajs_object_is_f64(double a, double b) {
    if (a != a && b != b) return 1;
    if (a == 0.0 && b == 0.0) {
        uint64_t ai, bi;
        memcpy(&ai, &a, sizeof(ai));
        memcpy(&bi, &b, sizeof(bi));
        return (ai == bi) ? 1 : 0;
    }
    return (a == b) ? 1 : 0;
}

/* T-09.d (v0.4.0) — `Object.freeze(obj)` sets the FROZEN bit in
 * the universal heap header's flags field; returns the same obj
 * pointer (Object.freeze returns its argument per spec). NULL
 * input is a no-op (defensive). The flag is consulted at every
 * field-write site emitted by ssa_lower's Assign-Member arm.
 *
 * STATIC_LITERAL guard (v0.4.0 fix): static-literal blocks
 * (string literals, freshly-constructed `[1,2,3]` after escape
 * analysis) live in `.rodata`; writing the FROZEN bit there
 * SIGBUSs. Per ES2015 spec Object.freeze is a no-op on values
 * that aren't extensible — static literals already aren't
 * extensible by tr's design — so silently skipping the bit set
 * matches both the spec and prevents the crash. test262
 * 15.2.3.9-1-3 / 15.2.3.9-1-4 / 15.2.3.9-2-d-1 cover this. */
void *__torajs_obj_freeze(void *p) {
    if (p == NULL) return NULL;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (h->flags & __TORAJS_FLAG_STATIC_LITERAL) return p;
    h->flags |= __TORAJS_FLAG_FROZEN;
    return p;
}

/* `Object.isFrozen(obj)` — reads the FROZEN bit. Returns 0 / 1
 * (matches the `_Bool` ABI tr's Bool intrinsics use). Static
 * literals are conceptually frozen (immutable rodata), so report
 * `true` for them — matches what test262 expects for primitives. */
_Bool __torajs_obj_is_frozen(const void *p) {
    if (p == NULL) return 0;
    const __torajs_heap_header_t *h = (const __torajs_heap_header_t *)p;
    if (h->flags & __TORAJS_FLAG_STATIC_LITERAL) return 1;
    return (h->flags & __TORAJS_FLAG_FROZEN) != 0;
}

/* Mutation guard called by ssa_lower at every Member-target Assign
 * site (`obj.field = value`). If the FROZEN bit is set, panics with
 * a TypeError-shaped message — matching bun's strict-mode behavior
 * (TypeScript files run in strict mode in bun, throwing
 * "Attempted to assign to readonly property"). NULL passes through
 * (defensive — assigning to a Nullable target hits the null-deref
 * panic elsewhere). */
void __torajs_obj_check_not_frozen(const void *p) {
    if (p == NULL) return;
    const __torajs_heap_header_t *h = (const __torajs_heap_header_t *)p;
    if (h->flags & __TORAJS_FLAG_FROZEN) {
        /* P7.4-frozen — real catchable TypeError instead of process
         * abort (spec §10.1.5 OrdinarySet: strict assignment to a
         * non-writable property throws). Mirrors a-2's dynobj
         * writable=false path. torajs_throw_type_error RETURNS (only
         * arms the throw slot) — bail now; ssa_lower emits an
         * emit_throw_check(None) right after this call which diverts
         * to the user's try/catch (or propagates) BEFORE the field
         * store, so the illegal mutation never happens. Prefix
         * stripped: .message is the bare text, .name is "TypeError". */
        __torajs_throw_type_error("Attempted to assign to readonly property");
        return;
    }
}

/* T-13.a (v0.4.0) — Symbol value runtime.
 *
 * Layout: 16 bytes — [hdr 8: refcount/type_tag=SYMBOL/flags]
 *                    [desc: void* @ offset 8]
 *
 * Each Symbol() call allocates a fresh heap block — Symbol identity
 * is pointer identity (`Symbol('x') === Symbol('x')` is false; same
 * Symbol compared by identity is true). The description (if any)
 * is an owning rc'd Str ref; NULL means undefined description.
 * Drop dec's the desc str via __torajs_str_drop. */

#define __TORAJS_SYMBOL_DESC_OFF  8
#define __TORAJS_SYMBOL_SIZE      16

void *__torajs_symbol_alloc(void *desc) {
    uint8_t *p = (uint8_t *)malloc(__TORAJS_SYMBOL_SIZE);
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_SYMBOL;
    h->flags = 0;
    /* Symbol owns its desc str — bump rc so the Symbol's lifetime
     * doesn't depend on the original literal's. NULL desc passes
     * through (rc_inc no-ops on NULL). */
    __torajs_rc_inc(desc);
    *(void **)(p + __TORAJS_SYMBOL_DESC_OFF) = desc;
    return p;
}

void __torajs_symbol_drop(void *p) {
    if (p == NULL) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (h->flags & __TORAJS_FLAG_STATIC_LITERAL) return;
    if (!__torajs_rc_dec(p)) return;
    void *desc = *(void **)((uint8_t *)p + __TORAJS_SYMBOL_DESC_OFF);
    if (desc != NULL) {
        __torajs_str_drop(desc);
    }
    free(p);
}

/* V3-18 m1.h.47 — Symbol.prototype.toString() returns
 * "Symbol(<desc>)" / "Symbol()" — same shape as the print helper
 * but builds a fresh Str instead of writing to stdout. */
void *__torajs_symbol_to_str(const void *p) {
    if (p == NULL) {
        const char *u = "undefined";
        size_t n = 9;
        uint8_t *r = __torajs_str_alloc_pooled(n);
        memcpy(__TORAJS_STR_DATA(r), u, n);
        return r;
    }
    void *desc = *(void *const *)((const uint8_t *)p + __TORAJS_SYMBOL_DESC_OFF);
    uint64_t desc_len = desc ? __TORAJS_STR_LEN(desc) : 0;
    /* "Symbol(" + desc + ")" */
    uint64_t total = 8 + desc_len;
    uint8_t *r = __torajs_str_alloc_pooled(total);
    uint8_t *dst = __TORAJS_STR_DATA(r);
    memcpy(dst, "Symbol(", 7);
    if (desc_len) {
        memcpy(dst + 7, __TORAJS_STR_CDATA(desc), (size_t)desc_len);
    }
    dst[7 + desc_len] = ')';
    return r;
}

/* V3-18 m1.h.47 — Symbol.prototype.description returns the desc
 * string the Symbol was created with, or null if Symbol() was
 * called with no arg. The desc is heap-shared with the Symbol;
 * caller does an rc_inc to take ownership (matches the property-
 * read convention for refcounted fields). */
void *__torajs_symbol_description(const void *p) {
    if (p == NULL) return NULL;
    void *desc = *(void *const *)((const uint8_t *)p + __TORAJS_SYMBOL_DESC_OFF);
    if (desc != NULL) {
        __torajs_rc_inc(desc);
    }
    return desc;
}

/* console.log dispatch for Type::Symbol — prints "Symbol(<desc>)"
 * or "Symbol()" when desc is NULL. Matches bun's format. */
void __torajs_symbol_print(const void *p) {
    if (p == NULL) {
        fputs("undefined\n", stdout);
        return;
    }
    void *desc = *(void *const *)((const uint8_t *)p + __TORAJS_SYMBOL_DESC_OFF);
    fputs("Symbol(", stdout);
    if (desc != NULL) {
        uint64_t len = __TORAJS_STR_LEN(desc);
        if (len) {
            fwrite(__TORAJS_STR_CDATA(desc), 1, (size_t)len, stdout);
        }
    }
    fputs(")\n", stdout);
}

/* T-13.b (v0.4.0) — global Symbol registry for Symbol.for / keyFor.
 *
 * Linear-scan array of Symbol ptrs (no separate key array — the key
 * is the Symbol's `desc` field, derivable). 256-slot cap covers any
 * realistic test262 / user-code workload; overflow panics with a
 * clear message rather than silently truncating.
 *
 * Refcount accounting:
 *   - hit: existing sym's rc bumped for the caller; registry keeps
 *     its own ref untouched
 *   - miss: alloc fresh sym (rc=1, caller-owned), then rc_inc to
 *     hold the registry's ref (rc=2 → caller drops to 1, registry
 *     to 1 on caller's drop)
 */

#define __TORAJS_SYMBOL_REG_MAX  256
static void *symbol_reg_[__TORAJS_SYMBOL_REG_MAX];
static int symbol_reg_count_ = 0;

void *__torajs_symbol_for(void *key) {
    if (key == NULL) {
        __torajs_panic("TypeError: Symbol.for requires a string key");
    }
    /* Scan existing entries by str_eq on the symbol's desc field. */
    for (int i = 0; i < symbol_reg_count_; i++) {
        void *sym = symbol_reg_[i];
        void *desc = *(void **)((uint8_t *)sym + __TORAJS_SYMBOL_DESC_OFF);
        if (desc != NULL
            && __torajs_str_eq((const uint8_t *)desc, (const uint8_t *)key))
        {
            __torajs_rc_inc(sym);
            return sym;
        }
    }
    if (symbol_reg_count_ >= __TORAJS_SYMBOL_REG_MAX) {
        __torajs_panic("Symbol.for registry full (>256 unique keys)");
    }
    /* Miss — alloc fresh sym (rc=1) + bump rc for registry's ref. */
    void *sym = __torajs_symbol_alloc(key);
    __torajs_rc_inc(sym);
    symbol_reg_[symbol_reg_count_++] = sym;
    return sym;
}

/* Symbol.keyFor(s) — returns the registered key string if s is in
 * the registry; NULL otherwise (caller's Nullable<String> slot maps
 * to bun's `undefined`). Returned Str is rc'd for the caller. */
void *__torajs_symbol_key_for(void *sym) {
    if (sym == NULL) return NULL;
    for (int i = 0; i < symbol_reg_count_; i++) {
        if (symbol_reg_[i] == sym) {
            void *desc = *(void **)((uint8_t *)sym + __TORAJS_SYMBOL_DESC_OFF);
            __torajs_rc_inc(desc);
            return desc;
        }
    }
    return NULL;
}

/* T-13.c (v0.4.0) — well-known Symbol singletons. Process-level
 * cached pointers, lazy-init on first access. Each getter rc_inc's
 * the singleton for the caller; the cache itself holds a "permanent"
 * ref (never freed, intentional — these live for process lifetime).
 *
 * Description string is constructed via __torajs_str_alloc_pooled +
 * memcpy from a C literal. for-of integration via
 * `[Symbol.iterator]()` method dispatch lands with v0.5 (alongside
 * async/await + iterator protocol substrate). */

static void *well_known_iterator_ = NULL;
static void *well_known_async_iterator_ = NULL;
static void *well_known_to_primitive_ = NULL;

static void *make_str_literal_(const char *s, uint64_t len) {
    uint8_t *p = __torajs_str_alloc_pooled(len);
    if (len) memcpy(p + __TORAJS_STR_HDR_SIZE, s, (size_t)len);
    return p;
}

void *__torajs_symbol_iterator(void) {
    if (well_known_iterator_ == NULL) {
        void *desc = make_str_literal_("Symbol.iterator", 15);
        well_known_iterator_ = __torajs_symbol_alloc(desc);
        __torajs_str_drop(desc); /* symbol_alloc rc_inc'd it */
    }
    __torajs_rc_inc(well_known_iterator_);
    return well_known_iterator_;
}

void *__torajs_symbol_async_iterator(void) {
    if (well_known_async_iterator_ == NULL) {
        void *desc = make_str_literal_("Symbol.asyncIterator", 20);
        well_known_async_iterator_ = __torajs_symbol_alloc(desc);
        __torajs_str_drop(desc);
    }
    __torajs_rc_inc(well_known_async_iterator_);
    return well_known_async_iterator_;
}

void *__torajs_symbol_to_primitive(void) {
    if (well_known_to_primitive_ == NULL) {
        void *desc = make_str_literal_("Symbol.toPrimitive", 18);
        well_known_to_primitive_ = __torajs_symbol_alloc(desc);
        __torajs_str_drop(desc);
    }
    __torajs_rc_inc(well_known_to_primitive_);
    return well_known_to_primitive_;
}

/* `__torajs_arr_push_unchecked` is inkwell-defined and exported as a
 * regular extern symbol; declare it here so the C runtime can call it
 * from split's pre-sized fast path (skips per-push capacity check +
 * potential realloc). */
void __torajs_arr_push_unchecked(void *arr, int64_t val);

/* Phase Substr.B — split returns Array<Substr> (view-substrings).
 *
 * Each output substring is a 32-byte view holding (parent_ptr, offset,
 * len), referencing slices of the source `s`. Zero byte memcpy across
 * the entire split — the per-iter allocation cost shrinks from
 * `N substring malloc + N memcpy` to `N view malloc` (no memcpy).
 *
 * For the empty-separator MVP we still return a single materialized
 * OWNED Str (mismatched element type vs the substring path; the
 * downstream array handler tolerates Str inside an Array<Substr> as
 * long as drop dispatches via Substr — but we don't currently emit
 * mixed-type slots, so just create a view of the whole source instead).
 */
/* Allocate the str_split output as a single block:
 *
 *   [arr_hdr:24][N*8 ptr slots][N*32 inline substr structs]
 *
 * Each slot[i] holds the address of the inline substr struct at
 * substrs_base + i*32. Each substr is marked INLINE in its flags so
 * substr_drop only dec's parent (no per-substr free). Final arr_drop
 * reclaims the entire block in one free(). Cuts the malloc count of
 * a split from N+1 (one arr alloc + N substr allocs) to exactly 1. */
static inline void __torajs_split_init_inline(
    uint8_t *substr_slot,
    void **arr_ptr_slot,
    const uint8_t *parent,
    uint64_t offset,
    uint64_t len
) {
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)substr_slot;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_STR;
    h->flags = __TORAJS_FLAG_SUBSTR_INLINE;
    __TORAJS_SUBSTR_LEN(substr_slot) = len;
    *(const uint8_t **)(substr_slot + __TORAJS_SUBSTR_PARENT_OFF) = parent;
    *(uint64_t *)(substr_slot + __TORAJS_SUBSTR_OFFSET_OFF) = offset;
    __torajs_rc_inc((void *)parent);
    *arr_ptr_slot = substr_slot;
}

void *__torajs_str_split(const uint8_t *s, const uint8_t *sep) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint64_t sep_len = __TORAJS_STR_LEN(sep);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    const uint8_t *sep_data = __TORAJS_STR_CDATA(sep);

    /* Pass 1 — count occurrences (out_count = matches + 1). Empty
     * separator splits per-char (TS spec: `"ab".split("") === ["a","b"]`),
     * yielding s_len single-byte substrings. */
    uint64_t matches = 0;
    uint64_t out_count;
    if (sep_len == 0) {
        out_count = s_len;
    } else if (sep_len == 1) {
        /* Hot path: byte scan. Most splits are " ", ",", "\n" etc. */
        uint8_t b = sep_data[0];
        for (uint64_t k = 0; k < s_len; k++) {
            if (s_data[k] == b) matches++;
        }
        out_count = matches + 1;
    } else if (sep_len <= s_len) {
        uint64_t i = 0;
        while (i + sep_len <= s_len) {
            if (memcmp(s_data + i, sep_data, (size_t)sep_len) == 0) {
                matches++;
                i += sep_len;
            } else {
                i++;
            }
        }
        out_count = matches + 1;
    } else {
        out_count = 1;
    }

    /* Single-block alloc — pool-aware. SPLIT flag tagged so
     * __torajs_arr_free routes free path to the split pool. */
    uint64_t slots_size = out_count * 8;
    uint8_t *arr = split_block_alloc_(out_count);
    __torajs_heap_header_t *ah = (__torajs_heap_header_t *)arr;
    ah->refcount = 1;
    ah->type_tag = __TORAJS_TAG_ARR;
    ah->flags = __TORAJS_FLAG_SPLIT_BLOCK;
    __TORAJS_ARR_LEN(arr) = out_count;
    __TORAJS_ARR_CAP(arr) = (uint32_t)out_count;
    __TORAJS_ARR_HEAD(arr) = 0;  /* T-13.5 — split block builds fresh */
    uint8_t *substrs_base = arr + __TORAJS_ARR_HDR_SIZE + slots_size;
    void **slots = (void **)(arr + __TORAJS_ARR_HDR_SIZE);

    if (sep_len == 0) {
        for (uint64_t k = 0; k < s_len; k++) {
            __torajs_split_init_inline(
                substrs_base + k * __TORAJS_SUBSTR_SIZE,
                &slots[k],
                s, k, 1
            );
        }
        return arr;
    }

    /* Pass 2 — fill substrs + slots inline. */
    uint64_t ix = 0;
    uint64_t start = 0;
    if (sep_len == 1) {
        uint8_t b = sep_data[0];
        for (uint64_t k = 0; k < s_len; k++) {
            if (s_data[k] == b) {
                __torajs_split_init_inline(
                    substrs_base + ix * __TORAJS_SUBSTR_SIZE,
                    &slots[ix],
                    s, start, k - start
                );
                ix++;
                start = k + 1;
            }
        }
    } else {
        uint64_t i = 0;
        while (i + sep_len <= s_len) {
            if (memcmp(s_data + i, sep_data, (size_t)sep_len) == 0) {
                __torajs_split_init_inline(
                    substrs_base + ix * __TORAJS_SUBSTR_SIZE,
                    &slots[ix],
                    s, start, i - start
                );
                ix++;
                i += sep_len;
                start = i;
            } else {
                i += 1;
            }
        }
    }
    __torajs_split_init_inline(
        substrs_base + ix * __TORAJS_SUBSTR_SIZE,
        &slots[ix],
        s, start, s_len - start
    );
    return arr;
}

/* ============================================================
 * SplitIter — zero-alloc iterator counterpart of __torajs_str_split.
 *
 * State lives in a 48-byte struct that the caller stack-allocates.
 * Each `next()` call writes the next yielded substring into a
 * 32-byte caller-provided slot and returns 1, or 0 once all tokens
 * (including the trailing one) have been yielded. Used by the
 * `for-of expr.split(s)` lowering and the SSA-level
 * `for-i + .length` rewrite pass — both consume substrings
 * sequentially, so eager Array<Substr> materialization is pure
 * overhead.
 *
 * Lifetime:
 *   - init() bumps parent's refcount once. drop() decrements once.
 *     The iter holds the only iter-side share; per-yield substrs
 *     are borrows that share parent's lifetime.
 *   - sep is borrowed (no rc bump). Caller must keep sep alive
 *     for the iter's lifetime. The current SSA lowering only
 *     emits the iter form when sep is a STATIC_LITERAL (.rodata
 *     global with infinite refcount), so this is naturally
 *     satisfied.
 *   - The yielded out_substr carries STATIC_LITERAL flag so any
 *     stray rc_inc / rc_dec / substr_drop on it no-ops. Bytes
 *     stay valid as long as parent is alive (handled by iter's
 *     parent rc).
 * ============================================================ */

typedef struct {
    const uint8_t *parent;       /* *Str — owns one iter-side ref */
    uint64_t parent_len;         /* cached __TORAJS_STR_LEN(parent) */
    const uint8_t *sep_data;     /* *Str CDATA — borrowed */
    uint64_t sep_len;
    uint64_t pos;                /* current scan position into parent */
    uint8_t exhausted;           /* 1 once trailing token has been yielded */
    uint8_t pad[7];              /* total 48B, 8-byte aligned */
} __torajs_split_iter_t;

void __torajs_split_iter_init(
    __torajs_split_iter_t *iter,
    const uint8_t *parent,
    const uint8_t *sep
) {
    iter->parent = parent;
    iter->parent_len = __TORAJS_STR_LEN(parent);
    iter->sep_data = __TORAJS_STR_CDATA(sep);
    iter->sep_len = __TORAJS_STR_LEN(sep);
    iter->pos = 0;
    iter->exhausted = 0;
    __torajs_rc_inc((void *)parent);
}

void __torajs_split_iter_drop(__torajs_split_iter_t *iter) {
    if (__torajs_rc_dec((void *)iter->parent)) {
        __torajs_str_free((uint8_t *)iter->parent);
    }
}

/* `__torajs_split_iter_next` body is now defined directly in inkwell
 * IR (ssa_inkwell.rs `define_split_iter_next`) so LLVM can inline
 * the byte scan + emit_substr into the caller's loop. The IR
 * version is logically identical to what was here; if the inkwell
 * side ever needs to fall back to a C call, restore this body and
 * change the dispatch in pass C. */

void *__torajs_arr_join(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t sep_len = __TORAJS_STR_LEN(sep);
    const uint8_t *sep_data = __TORAJS_STR_CDATA(sep);

    if (len == 0) {
        return __torajs_str_alloc_pooled(0);
    }

    /* pass 1: total = sum(elem.len) + sep_len * (len - 1) */
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        const uint8_t *elem = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(arr, i);
        total += __TORAJS_STR_LEN(elem);
    }
    total += sep_len * (len - 1);

    /* pass 2: copy */
    uint8_t *p = __torajs_str_alloc_pooled(total);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    uint64_t cursor = 0;
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0 && sep_len) {
            memcpy(p_data + cursor, sep_data, (size_t)sep_len);
            cursor += sep_len;
        }
        const uint8_t *elem = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(arr, i);
        uint64_t elem_len = __TORAJS_STR_LEN(elem);
        if (elem_len) {
            memcpy(p_data + cursor, __TORAJS_STR_CDATA(elem), (size_t)elem_len);
            cursor += elem_len;
        }
    }
    return p;
}

/* V3-18 m1.h.43 — Array<i64>.join(sep). Stringify each i64 element
 * with snprintf, memcpy with sep between. Per JS spec §22.1.3.13:
 * elements ToString'd then joined. */
void *__torajs_arr_join_i64(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t sep_len = __TORAJS_STR_LEN(sep);
    const uint8_t *sep_data = __TORAJS_STR_CDATA(sep);
    if (len == 0) return __torajs_str_alloc_pooled(0);
    char buf[24];  /* max i64 = 20 digits + sign + NUL */
    /* Pass 1: total length. */
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        int64_t e = *(const int64_t *)__TORAJS_ARR_CSLOT(arr, i);
        int n = snprintf(buf, sizeof(buf), "%lld", (long long)e);
        if (n < 0) n = 0;
        total += (uint64_t)n;
    }
    total += sep_len * (len - 1);
    uint8_t *p = __torajs_str_alloc_pooled(total);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    uint64_t cursor = 0;
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0 && sep_len) {
            memcpy(p_data + cursor, sep_data, (size_t)sep_len);
            cursor += sep_len;
        }
        int64_t e = *(const int64_t *)__TORAJS_ARR_CSLOT(arr, i);
        int n = snprintf(buf, sizeof(buf), "%lld", (long long)e);
        if (n < 0) n = 0;
        memcpy(p_data + cursor, buf, (size_t)n);
        cursor += (uint64_t)n;
    }
    return p;
}

/* V3-18 m1.h.43 — Array<f64>.join(sep). Same shape as the i64 path
 * but using torajs_f64_shortest for spec-correct number → string. */
void *__torajs_arr_join_f64(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t sep_len = __TORAJS_STR_LEN(sep);
    const uint8_t *sep_data = __TORAJS_STR_CDATA(sep);
    if (len == 0) return __torajs_str_alloc_pooled(0);
    char buf[32];
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        double e = *(const double *)__TORAJS_ARR_CSLOT(arr, i);
        if (e != e) {
            total += 3;  /* "NaN" */
        } else if (e == 1.0/0.0) {
            total += 8;  /* "Infinity" */
        } else if (e == -1.0/0.0) {
            total += 9;  /* "-Infinity" */
        } else {
            int n = torajs_f64_shortest(e, buf, sizeof(buf));
            if (n < 0) n = 0;
            total += (uint64_t)n;
        }
    }
    total += sep_len * (len - 1);
    uint8_t *p = __torajs_str_alloc_pooled(total);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    uint64_t cursor = 0;
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0 && sep_len) {
            memcpy(p_data + cursor, sep_data, (size_t)sep_len);
            cursor += sep_len;
        }
        double e = *(const double *)__TORAJS_ARR_CSLOT(arr, i);
        if (e != e) {
            memcpy(p_data + cursor, "NaN", 3);
            cursor += 3;
        } else if (e == 1.0/0.0) {
            memcpy(p_data + cursor, "Infinity", 8);
            cursor += 8;
        } else if (e == -1.0/0.0) {
            memcpy(p_data + cursor, "-Infinity", 9);
            cursor += 9;
        } else {
            int n = torajs_f64_shortest(e, buf, sizeof(buf));
            if (n < 0) n = 0;
            memcpy(p_data + cursor, buf, (size_t)n);
            cursor += (uint64_t)n;
        }
    }
    return p;
}

/* V3-18 m1.h.43 — Array<bool>.join(sep). Each element is "true" /
 * "false". */
void *__torajs_arr_join_bool(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t sep_len = __TORAJS_STR_LEN(sep);
    const uint8_t *sep_data = __TORAJS_STR_CDATA(sep);
    if (len == 0) return __torajs_str_alloc_pooled(0);
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        int64_t e = *(const int64_t *)__TORAJS_ARR_CSLOT(arr, i);
        total += e ? 4 : 5;
    }
    total += sep_len * (len - 1);
    uint8_t *p = __torajs_str_alloc_pooled(total);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    uint64_t cursor = 0;
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0 && sep_len) {
            memcpy(p_data + cursor, sep_data, (size_t)sep_len);
            cursor += sep_len;
        }
        int64_t e = *(const int64_t *)__TORAJS_ARR_CSLOT(arr, i);
        if (e) {
            memcpy(p_data + cursor, "true", 4);
            cursor += 4;
        } else {
            memcpy(p_data + cursor, "false", 5);
            cursor += 5;
        }
    }
    return p;
}

/* `Array<Substr>.join(sep)` — view-aware joiner. Each element is a
 * Substr whose bytes live at `parent.bytes + offset`. Two-pass:
 * (1) sum view lengths to size the output, (2) memcpy each view's
 * bytes (resolved through parent_ptr) into the output, separator
 * between. Used when split's Array<Substr> result feeds straight
 * into join — keeps the zero-copy property of split intact. */
void *__torajs_arr_join_substr(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t sep_len = __TORAJS_STR_LEN(sep);
    const uint8_t *sep_data = __TORAJS_STR_CDATA(sep);

    if (len == 0) {
        return __torajs_str_alloc_pooled(0);
    }

    /* pass 1: total = sum(view.len) + sep_len * (len - 1) */
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        const uint8_t *v = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(arr, i);
        total += __TORAJS_SUBSTR_LEN(v);
    }
    total += sep_len * (len - 1);

    /* pass 2: copy */
    uint8_t *p = __torajs_str_alloc_pooled(total);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    uint64_t cursor = 0;
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0 && sep_len) {
            memcpy(p_data + cursor, sep_data, (size_t)sep_len);
            cursor += sep_len;
        }
        const uint8_t *v = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(arr, i);
        uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
        if (v_len) {
            const uint8_t *parent = *(const uint8_t *const *)(v + __TORAJS_SUBSTR_PARENT_OFF);
            uint64_t v_off = *(const uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF);
            memcpy(p_data + cursor, parent + __TORAJS_STR_HDR_SIZE + v_off, (size_t)v_len);
            cursor += v_len;
        }
    }
    return p;
}

/* `String.fromCharCode(n)` — single-char string from a code point,
 * truncated to byte (matches v0's byte-Str layout; non-ASCII would
 * need UTF-8 encoding). */
void *__torajs_str_from_char_code(int64_t n) {
    uint8_t *p = __torajs_str_alloc_pooled(1);
    __TORAJS_STR_DATA(p)[0] = (uint8_t)(n & 0xff);
    return p;
}

/* `arr.toReversed()` (ES2023) — non-mutating reverse. Single malloc +
 * reverse-direction byte-by-byte slot copy. Original untouched. */
void *__torajs_arr_to_reversed(const uint8_t *arr) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint8_t *p = arr_alloc_(len, len);
    for (uint64_t i = 0; i < len; i++) {
        *(uint64_t *)__TORAJS_ARR_SLOT(p, i)
            = *(const uint64_t *)__TORAJS_ARR_CSLOT(arr, len - 1 - i);
    }
    return p;
}

/* `arr.with(i, v)` (ES2023) — non-mutating index update. Single malloc
 * + memcpy + single slot overwrite. Negative `i` wraps via `len + i`.
 * Out-of-bounds `i` is UB (matches the unchecked-index convention used
 * elsewhere in tr's array runtime). */
void *__torajs_arr_with(const uint8_t *arr, int64_t i, int64_t v) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint8_t *p = arr_alloc_(len, len);  /* fresh, head=0 */
    if (len) memcpy(__TORAJS_ARR_DATA(p), __TORAJS_ARR_CSLOT(arr, 0), (size_t)len * 8);
    int64_t adj = i < 0 ? (int64_t)len + i : i;
    *(uint64_t *)__TORAJS_ARR_SLOT(p, adj) = (uint64_t)v;
    return p;
}

/* `s.substring(start, end)` — slice's pre-ES5 sibling. Diverges from
 * slice on two corners: negative inputs clamp to 0 (slice wraps to
 * len+n), and start > end gets silently swapped. After fixup both
 * indices clamp to [0, len], same as slice. */
void *__torajs_str_substring(const uint8_t *s, int64_t start, int64_t end) {
    uint64_t len = __TORAJS_STR_LEN(s);
    if (start < 0) start = 0;
    if (end < 0) end = 0;
    if (start > (int64_t)len) start = (int64_t)len;
    if (end > (int64_t)len) end = (int64_t)len;
    if (start > end) {
        int64_t tmp = start;
        start = end;
        end = tmp;
    }
    uint64_t new_len = (uint64_t)(end - start);
    uint8_t *p = __torajs_str_alloc_pooled(new_len);
    if (new_len) memcpy(__TORAJS_STR_DATA(p), __TORAJS_STR_CDATA(s) + start, (size_t)new_len);
    return p;
}

/* T-29.b — `Object.defineProperty(arr, "length", { value: v })` validation
 * per JS spec §9.4.2.4 ArraySetLength. The descriptor value must satisfy
 * ToUint32(v) === ToNumber(v); otherwise throws RangeError. Common
 * failing cases: negative number, NaN, fractional non-integer, value
 * outside [0, 2^32-1].
 *
 * tora's typed pack: tag 1=Bool, 2=I64, 3=F64-bits, 4=heap, 5=undef,
 * 0=null/other. The caller (ssa_lower defineProperty intercept) packs
 * the descriptor.value into (tag, value-as-i64) using the same table
 * the BinOp Any===concrete arm uses.
 *
 * On RangeError: stores the error string into the thread-local throw
 * slot via `__torajs_throw_set`. The ssa_lower-side `emit_throw_check`
 * after this call branches to the function's throw handler — try/catch
 * around the defineProperty call (the test262 / assert.throws shape)
 * catches it; without a handler the throw propagates to fn boundary.
 */
/* P2.4-a (2026-05-23 architecture-rewrite) — native-error factory
 * registry + `__torajs_throw_range_error` / `__torajs_throw_type
 * _error` cross-TU helpers + the internal `torajs_throw_native`
 * dispatch now provided by the Rust `torajs-throw` crate. C
 * definitions deleted from this file; the extern decls forward-
 * declared near the top (~line 1083) let in-file callers
 * (dynobj_set / dynobj_define / arr_set_length_validate) keep
 * resolving the public symbols at link time. The previous static
 * helpers `torajs_throw_range_error` / `torajs_throw_type_error`
 * are gone — call sites updated to use the public Rust names. */

void __torajs_arr_set_length_validate(int64_t tag, int64_t value) {
    /* Resolve descriptor.value to a JS Number. tora's typed pack:
     *   tag 1=Bool · 2=I64 · 3=F64-bits · 4=heap · 5=undef · 0=null/other.
     * Bool / null map to 0 or 1 (valid lengths); undefined and heap
     * objects map to NaN (invalid). */
    double n;
    switch (tag) {
        case 0: return;            /* null → ToNumber=0 → valid */
        case 1: return;            /* Bool 0/1 → valid */
        case 2: n = (double)value; break;
        case 3: { union { int64_t i; double d; } u = { .i = value }; n = u.d; break; }
        default: __torajs_throw_range_error("Invalid array length"); return;
    }
    /* Spec §9.4.2.4: throw if ToUint32(v) !== ToNumber(v). Equivalent
     * check: n must be a non-negative integer in [0, 2^32 - 1]. NaN /
     * Infinity / fractional / negative / overflow all fail. */
    if (n != n || n < 0.0 || n > 4294967295.0 || n != (double)(int64_t)n) {
        __torajs_throw_range_error("Invalid array length");
    }
}

/* T-49 — `s.substr(start, length)` (annexB legacy). Diverges from
 * slice / substring in two ways per JS spec §B.2.3.1:
 *   - Negative start wraps to `max(size + start, 0)` (slice wraps too;
 *     substring clamps to 0).
 *   - The 2nd arg is a *length*, not an end index — clamps to
 *     [0, size - start]. The caller passes `length = INT64_MAX` for
 *     the 1-arg form so this helper clamps to the remaining bytes.
 */
void *__torajs_str_substr(const uint8_t *s, int64_t start, int64_t length) {
    int64_t size = (int64_t)__TORAJS_STR_LEN(s);
    if (start < 0) start = size + start;
    if (start < 0) start = 0;
    if (start > size) start = size;
    int64_t avail = size - start;
    if (length > avail) length = avail;
    if (length < 0) length = 0;
    uint8_t *p = __torajs_str_alloc_pooled((uint64_t)length);
    if (length > 0) {
        memcpy(
            __TORAJS_STR_DATA(p),
            __TORAJS_STR_CDATA(s) + start,
            (size_t)length
        );
    }
    return p;
}

/* `Array.from(s)` over a string source — fresh `string[]` with one
 * single-byte string per byte of `s`. Mirrors `s.split("")` in JS but
 * scoped to tr's byte-Str layout (no UTF-16 / surrogate handling). */
void *__torajs_arr_from_string(const uint8_t *s) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    void *arr = __torajs_arr_alloc(s_len);
    for (uint64_t i = 0; i < s_len; i++) {
        uint8_t *p = __torajs_str_alloc_pooled(1);
        __TORAJS_STR_DATA(p)[0] = s_data[i];
        arr = __torajs_arr_push(arr, (int64_t)(intptr_t)p);
    }
    return arr;
}

/* `s.at(i)` — single-char string at index i, with negative-index wrap.
 * Returns the empty string if i is out of bounds (matches JS spec —
 * returning undefined would need Nullable<string>, not in v0). */
void *__torajs_str_at(const uint8_t *s, int64_t i) {
    uint64_t len = __TORAJS_STR_LEN(s);
    int64_t adj = i < 0 ? (int64_t)len + i : i;
    if (adj < 0 || adj >= (int64_t)len) {
        return __torajs_str_alloc_pooled(0);
    }
    uint8_t *p = __torajs_str_alloc_pooled(1);
    __TORAJS_STR_DATA(p)[0] = __TORAJS_STR_CDATA(s)[adj];
    return p;
}

/* `s.replace(needle, replacement)` — replace the FIRST occurrence of
 * `needle` in `s` with `replacement`. Returns a fresh string; the
 * original is untouched. JS spec accepts a regex needle; we only
 * support string needles in v0. If `needle` doesn't occur, returns a
 * fresh copy of `s` (so the caller can drop both inputs uniformly). */
void *__torajs_str_replace(const uint8_t *s, const uint8_t *needle, const uint8_t *repl) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint64_t n_len = __TORAJS_STR_LEN(needle);
    uint64_t r_len = __TORAJS_STR_LEN(repl);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    const uint8_t *n_data = __TORAJS_STR_CDATA(needle);
    const uint8_t *r_data = __TORAJS_STR_CDATA(repl);
    /* Find the first occurrence. memmem isn't portable across BSD/Linux
     * uniformly — manual search keeps the deps minimal. */
    int64_t found = -1;
    if (n_len == 0) {
        /* Empty needle — JS inserts at index 0. Returns repl + s. */
        found = 0;
    } else if (n_len <= s_len) {
        for (uint64_t i = 0; i + n_len <= s_len; i++) {
            if (memcmp(s_data + i, n_data, (size_t)n_len) == 0) {
                found = (int64_t)i;
                break;
            }
        }
    }
    if (found < 0) {
        /* Not found — return a fresh copy of s. */
        uint8_t *p = __torajs_str_alloc_pooled(s_len);
        if (s_len) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)s_len);
        return p;
    }
    uint64_t out_len = s_len - n_len + r_len;
    uint8_t *p = __torajs_str_alloc_pooled(out_len);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    if (found > 0) memcpy(p_data, s_data, (size_t)found);
    if (r_len) memcpy(p_data + (size_t)found, r_data, (size_t)r_len);
    uint64_t tail_off = (uint64_t)found + n_len;
    uint64_t tail_len = s_len - tail_off;
    if (tail_len) {
        memcpy(p_data + (uint64_t)found + r_len, s_data + tail_off, (size_t)tail_len);
    }
    return p;
}

/* `s.replaceAll(needle, replacement)` — every occurrence. Counts hits
 * with non-overlapping search (the standard JS behavior), pre-allocs
 * the exact result size, then does a single fill pass. */
void *__torajs_str_replace_all(const uint8_t *s, const uint8_t *needle, const uint8_t *repl) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint64_t n_len = __TORAJS_STR_LEN(needle);
    uint64_t r_len = __TORAJS_STR_LEN(repl);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    const uint8_t *n_data = __TORAJS_STR_CDATA(needle);
    const uint8_t *r_data = __TORAJS_STR_CDATA(repl);
    if (n_len == 0) {
        /* JS spec: empty needle on replaceAll throws TypeError. We
         * don't throw at the runtime layer — just return a copy. The
         * subset shouldn't trigger this path under a typical test. */
        uint8_t *p = __torajs_str_alloc_pooled(s_len);
        if (s_len) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)s_len);
        return p;
    }
    /* Pass 1 — count occurrences. */
    uint64_t hits = 0;
    if (n_len <= s_len) {
        uint64_t i = 0;
        while (i + n_len <= s_len) {
            if (memcmp(s_data + i, n_data, (size_t)n_len) == 0) {
                hits++;
                i += n_len;  /* non-overlapping */
            } else {
                i++;
            }
        }
    }
    if (hits == 0) {
        uint8_t *p = __torajs_str_alloc_pooled(s_len);
        if (s_len) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)s_len);
        return p;
    }
    /* out_len = s_len - hits*n_len + hits*r_len */
    uint64_t out_len = s_len + hits * (r_len > n_len ? (r_len - n_len) : 0)
                              - hits * (r_len < n_len ? (n_len - r_len) : 0);
    uint8_t *p = __torajs_str_alloc_pooled(out_len);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    /* Pass 2 — copy with substitutions. */
    uint64_t src_i = 0, dst_i = 0;
    while (src_i + n_len <= s_len) {
        if (memcmp(s_data + src_i, n_data, (size_t)n_len) == 0) {
            if (r_len) memcpy(p_data + dst_i, r_data, (size_t)r_len);
            dst_i += r_len;
            src_i += n_len;
        } else {
            p_data[dst_i] = s_data[src_i];
            dst_i++;
            src_i++;
        }
    }
    while (src_i < s_len) {
        p_data[dst_i] = s_data[src_i];
        dst_i++;
        src_i++;
    }
    return p;
}

/* Lookup ops (locale_compare / starts_with_from / ends_with_from /
 * index_of_from / includes_from / last_index_of_from / last_index_of)
 * moved to torajs-str::lookup (P3.1-d, 2026-05-23). Pure-Rust cores
 * mirror the byte-for-byte memcmp scans; the IR-side variants without
 * `_from` suffix (starts_with / ends_with / index_of / includes) remain
 * LLVM-IR-emitted in ssa_inkwell until P3.1-g consolidation. */
extern int64_t __torajs_str_locale_compare(const uint8_t *a, const uint8_t *b);
extern int64_t __torajs_str_starts_with_from(const uint8_t *s, const uint8_t *sub, int64_t pos);
extern int64_t __torajs_str_ends_with_from(const uint8_t *s, const uint8_t *sub, int64_t end);
extern int64_t __torajs_str_index_of_from(const uint8_t *s, const uint8_t *sub, int64_t from);
extern int64_t __torajs_str_includes_from(const uint8_t *s, const uint8_t *sub, int64_t from);
extern int64_t __torajs_str_last_index_of_from(const uint8_t *s, const uint8_t *sub, int64_t from);
extern int64_t __torajs_str_last_index_of(const uint8_t *s, const uint8_t *needle);

/* `JSON.stringify` — string-escape helper for the recursive ssa-lower
 * generator. Wraps `s` in `"..."` and replaces JSON-illegal control
 * chars and quote / backslash bytes. Single pass; pre-computes output
 * length for a single malloc. */
void *__torajs_json_quote_str(const uint8_t *s) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    uint64_t out = 2; /* surrounding quotes */
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s_data[i];
        if (c == '"' || c == '\\' || c == '\n' || c == '\r'
            || c == '\t' || c == '\b' || c == '\f') {
            out += 2;
        } else if (c < 0x20) {
            out += 6; /* \uXXXX */
        } else {
            out += 1;
        }
    }
    uint8_t *p = __torajs_str_alloc_pooled(out);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    p_data[0] = '"';
    uint64_t cur = 1;
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s_data[i];
        switch (c) {
            case '"':  p_data[cur++] = '\\'; p_data[cur++] = '"';  break;
            case '\\': p_data[cur++] = '\\'; p_data[cur++] = '\\'; break;
            case '\n': p_data[cur++] = '\\'; p_data[cur++] = 'n';  break;
            case '\r': p_data[cur++] = '\\'; p_data[cur++] = 'r';  break;
            case '\t': p_data[cur++] = '\\'; p_data[cur++] = 't';  break;
            case '\b': p_data[cur++] = '\\'; p_data[cur++] = 'b';  break;
            case '\f': p_data[cur++] = '\\'; p_data[cur++] = 'f';  break;
            default:
                if (c < 0x20) {
                    static const char hex[] = "0123456789abcdef";
                    p_data[cur++] = '\\'; p_data[cur++] = 'u';
                    p_data[cur++] = '0'; p_data[cur++] = '0';
                    p_data[cur++] = hex[(c >> 4) & 0xf];
                    p_data[cur++] = hex[c & 0xf];
                } else {
                    p_data[cur++] = c;
                }
        }
    }
    p_data[cur] = '"';
    return p;
}

/* `Math.random()` — uniform [0, 1). libc rand()/RAND_MAX scaled. Not
 * cryptographically secure; matches the JS spec's "implementation-
 * defined" wording for the simple use case. */
double __torajs_math_random(void) {
    return (double)rand() / ((double)RAND_MAX + 1.0);
}

/* `Math.imul(a, b)` — 32-bit signed integer multiplication, low 32
 * bits, sign-extended. Same shape as JS spec.
 */
int64_t __torajs_math_imul(int64_t a, int64_t b) {
    int32_t result = (int32_t)((uint32_t)((int32_t)a) * (uint32_t)((int32_t)b));
    return (int64_t)result;
}

/* `Math.clz32(x)` — count leading zeros of x's 32-bit unsigned
 * representation. Returns 32 if x is zero. */
int64_t __torajs_math_clz32(int64_t x) {
    uint32_t v = (uint32_t)((int32_t)x);
    if (v == 0) return 32;
    return (int64_t)__builtin_clz(v);
}

/* `Math.fround(x)` — round x to the nearest f32 then back to f64. */
double __torajs_math_fround(double x) {
    return (double)(float)x;
}

/* console.error / console.warn — stderr-routed primitives matching
 * console.log's three-way SSA dispatch. Same shape as the print_*
 * intrinsics but write to fd 2.
 */
void __torajs_print_i64_err(int64_t n) {
    fprintf(stderr, "%lld\n", (long long)n);
}
void __torajs_print_f64_err(double d) {
    fprintf(stderr, "%g\n", d);
}
void __torajs_print_bool_err(int64_t b) {
    fputs(b ? "true\n" : "false\n", stderr);
}
void __torajs_str_print_err(const uint8_t *s) {
    /* NULL guard — Nullable<Str> slots and uncaptured regex group slots
     * pass NULL through here. Print "null" to match console.log(null)
     * semantics rather than segfaulting on the len read. */
    if (s == NULL) {
        fputs("null\n", stderr);
        return;
    }
    uint64_t len = __TORAJS_STR_LEN(s);
    if (len) fwrite(__TORAJS_STR_CDATA(s), 1, (size_t)len, stderr);
    fputc('\n', stderr);
}

/* ============================================================
 * v0.3 #1 — fs module helpers. Synchronous file I/O surfaces:
 *   readFileSync(path) → string of file contents
 *   writeFileSync(path, data) → void (truncates)
 *   existsSync(path) → boolean
 *
 * Path is a NUL-terminated copy on the stack so we can pass it
 * straight to libc; tr's Str isn't NUL-terminated by default.
 * Files are read in one shot via fseek/ftell/fread; writes go
 * through a single fwrite. v0.3.b will add streaming readers /
 * appendFileSync / readdirSync / statSync; for now this trio
 * unlocks the most common CLI idioms (read input, write output,
 * existence check). Errors (file not found, permission denied,
 * etc.) currently abort with stderr — typed throw integration
 * comes in v0.3.b.
 * ============================================================ */

static void path_copy_to_buf(const void *path_str, char *buf, size_t bufsz) {
    const uint8_t *p = __TORAJS_STR_CDATA(path_str);
    uint64_t plen = __TORAJS_STR_LEN(path_str);
    if (plen >= bufsz) plen = bufsz - 1;
    memcpy(buf, p, (size_t)plen);
    buf[plen] = '\0';
}

void *__torajs_fs_read_file_sync(const void *path_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    FILE *f = fopen(path, "rb");
    if (!f) {
        char msg[4200];
        snprintf(msg, sizeof(msg), "not yet supported: fs.readFileSync open failed: %s", path);
        __torajs_panic(msg);
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        __torajs_panic("not yet supported: fs.readFileSync seek failed");
    }
    long sz = ftell(f);
    if (sz < 0) {
        fclose(f);
        __torajs_panic("not yet supported: fs.readFileSync ftell failed");
    }
    rewind(f);
    uint8_t *out = __torajs_str_alloc_pooled((uint64_t)sz);
    size_t got = fread(out + __TORAJS_STR_HDR_SIZE, 1, (size_t)sz, f);
    fclose(f);
    if (got != (size_t)sz) {
        __torajs_panic("not yet supported: fs.readFileSync short read");
    }
    return out;
}

void __torajs_fs_write_file_sync(const void *path_str, const void *data_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    FILE *f = fopen(path, "wb");
    if (!f) {
        char msg[4200];
        snprintf(msg, sizeof(msg), "not yet supported: fs.writeFileSync open failed: %s", path);
        __torajs_panic(msg);
    }
    const uint8_t *d = __TORAJS_STR_CDATA(data_str);
    uint64_t dlen = __TORAJS_STR_LEN(data_str);
    size_t put = fwrite(d, 1, (size_t)dlen, f);
    fclose(f);
    if (put != (size_t)dlen) {
        __torajs_panic("not yet supported: fs.writeFileSync short write");
    }
}

_Bool __torajs_fs_exists_sync(const void *path_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    FILE *f = fopen(path, "rb");
    if (!f) return 0;
    fclose(f);
    return 1;
}

void __torajs_fs_append_file_sync(const void *path_str, const void *data_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    FILE *f = fopen(path, "ab");
    if (!f) {
        char msg[4200];
        snprintf(msg, sizeof(msg), "not yet supported: fs.appendFileSync open failed: %s", path);
        __torajs_panic(msg);
    }
    const uint8_t *d = __TORAJS_STR_CDATA(data_str);
    uint64_t dlen = __TORAJS_STR_LEN(data_str);
    size_t put = fwrite(d, 1, (size_t)dlen, f);
    fclose(f);
    if (put != (size_t)dlen) {
        __torajs_panic("not yet supported: fs.appendFileSync short write");
    }
}

/* ============================================================
 * v0.3 #3 — process surface (minimum). Synchronous shape:
 *   process.exit(code)  → calls libc exit (no return)
 *   process.cwd()       → string of getcwd()
 *   process.platform    → static "darwin" / "linux" / etc.
 *
 * argv / env are deferred to v0.3.b (need to thread main()'s argc/argv
 * into the user program — extra plumbing through ssa_lower's main entry).
 * ============================================================ */

#include <unistd.h>
#include <sys/stat.h>

void __torajs_fs_unlink_sync(const void *path_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    if (unlink(path) != 0) {
        char msg[4200];
        snprintf(msg, sizeof(msg), "not yet supported: fs.unlinkSync failed: %s", path);
        __torajs_panic(msg);
    }
}

void __torajs_fs_mkdir_sync(const void *path_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    if (mkdir(path, 0755) != 0) {
        /* JS spec throws on existing dir unless `recursive: true` —
         * we mirror by aborting (typed-throw is Phase 2.0c). */
        char msg[4200];
        snprintf(msg, sizeof(msg), "not yet supported: fs.mkdirSync failed: %s", path);
        __torajs_panic(msg);
    }
}

/* T-18.c (v0.5.0) — fs size probe used by `Bun.file(p).size` and
 * future `fs.statSync(p).size`. Returns the file's byte size or
 * -1 on error (missing / unreadable / not a regular file). Doesn't
 * panic — the bun-spec `size` getter is synchronous AND error-free
 * (returns 0 for missing files in bun; tr returns -1 to make
 * "missing" distinguishable). Spec-strict 0-on-missing lands when
 * we wire a typed fs.exists check at the call site. */
int64_t __torajs_fs_size_sync(const void *path_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    struct stat st;
    if (stat(path, &st) != 0) return -1;
    if (!S_ISREG(st.st_mode)) return -1;
    return (int64_t)st.st_size;
}

/* T-18.b (v0.5.0) — fs.readdirSync(path) returns Array<string> with
 * one entry per directory child (excluding `.` and `..`, matching
 * bun / node spec). Caller owns the result Array; each Str element
 * has rc=1. Order matches the OS's readdir() ordering (typically
 * inode-order on ext4 / btrfs, deterministic on test setups). */
#include <dirent.h>
void *__torajs_fs_readdir_sync(const void *path_str) {
    char path[4096];
    path_copy_to_buf(path_str, path, sizeof(path));
    DIR *d = opendir(path);
    if (!d) {
        char msg[4200];
        snprintf(msg, sizeof(msg), "not yet supported: fs.readdirSync open failed: %s", path);
        __torajs_panic(msg);
    }
    void *arr = __torajs_arr_alloc(0);
    struct dirent *ent;
    while ((ent = readdir(d)) != NULL) {
        const char *name = ent->d_name;
        if (name[0] == '.' && (name[1] == '\0'
            || (name[1] == '.' && name[2] == '\0')))
        {
            continue; /* skip "." and ".." per spec */
        }
        uint64_t nlen = strlen(name);
        uint8_t *s = __torajs_str_alloc_pooled(nlen);
        if (nlen) memcpy(s + __TORAJS_STR_HDR_SIZE, name, (size_t)nlen);
        arr = __torajs_arr_push(arr, (int64_t)(intptr_t)s);
    }
    closedir(d);
    return arr;
}

void __torajs_process_exit(int64_t code) {
    exit((int)code);
}

void *__torajs_process_cwd(void) {
    char buf[4096];
    if (getcwd(buf, sizeof(buf)) == NULL) {
        uint8_t *empty = __torajs_str_alloc_pooled(0);
        return empty;
    }
    uint64_t len = strlen(buf);
    uint8_t *out = __torajs_str_alloc_pooled(len);
    memcpy(out + __TORAJS_STR_HDR_SIZE, buf, (size_t)len);
    return out;
}

/* `process.env.NAME` lookup — calls libc getenv on the NUL-copy of
 * the name. Returns a freshly-allocated Str of the env value, or
 * NULL pointer if the var isn't set (caller's slot is Nullable<Str>;
 * tr's `undefined` maps to NULL via the undefined→null bridge so
 * `process.env.X === undefined` round-trips correctly). */
void *__torajs_process_getenv(const void *name_str) {
    char name[256];
    path_copy_to_buf(name_str, name, sizeof(name));
    const char *v = getenv(name);
    if (!v) return NULL;
    uint64_t len = strlen(v);
    uint8_t *out = __torajs_str_alloc_pooled(len);
    memcpy(out + __TORAJS_STR_HDR_SIZE, v, (size_t)len);
    return out;
}

/* v0.3 #3.c — argv plumbing.
 * `__torajs_argv_init` is called once at the start of LLVM-emitted
 * `main(argc, argv)` (declare_ssa_fn widens the signature; the
 * FnLower main-entry hook emits the call). The captured argc/argv
 * stay valid for the program's lifetime — they live on the kernel-
 * provided stack frame, which outlives any user code. */
static int g_argc = 0;
static char **g_argv = NULL;

void __torajs_argv_init(int32_t argc, char **argv) {
    g_argc = (int)argc;
    g_argv = argv;
}

void *__torajs_process_argv(void) {
    void *out = __torajs_arr_alloc((uint64_t)g_argc);
    for (int i = 0; i < g_argc; i++) {
        const char *s = g_argv[i];
        uint64_t len = strlen(s);
        uint8_t *str_v = __torajs_str_alloc_pooled(len);
        memcpy(str_v + __TORAJS_STR_HDR_SIZE, s, (size_t)len);
        out = __torajs_arr_push(out, (int64_t)(intptr_t)str_v);
    }
    return out;
}

void *__torajs_process_platform(void) {
#if defined(__APPLE__)
    static const char p[] = "darwin";
#elif defined(__linux__)
    static const char p[] = "linux";
#elif defined(_WIN32)
    static const char p[] = "win32";
#else
    static const char p[] = "unknown";
#endif
    uint64_t len = sizeof(p) - 1;
    uint8_t *out = __torajs_str_alloc_pooled(len);
    memcpy(out + __TORAJS_STR_HDR_SIZE, p, (size_t)len);
    return out;
}

/* T-03 (v0.3.0) — synchronous stdio.
 *
 * `process.stdout.write(s)` and `process.stderr.write(s)` write the
 * raw Str bytes (no trailing newline, no formatting) and return the
 * number of bytes accepted by the OS (matches bun.write_returns_int).
 * On a short write or write error the helpers panic — JS spec says
 * `write` returns false / boolean, but tr's typed-throw substrate
 * doesn't yet model the success / failure return; aborting on
 * failure preserves "tr-accepted parity = 100%" since any caller that
 * relied on the return value would already be a typed-throw site
 * we can't represent.
 *
 * `process.stdin.read()` reads stdin to EOF and returns one Str. Sync
 * by design (the v0.5 async fs surface adds the streaming variant).
 * No size limit beyond Str's i64 length bound; chunked into 4 KB
 * reads to avoid an extra full-buffer alloc when stdin is small.
 */
/* Bun signature: `process.stdout.write(s) → boolean` (true on
 * success, false on backpressure / error). tr panics on short write
 * — typed-throw substrate for graceful failure lands with v0.3
 * #1.b — so the only return that reaches user code is `true`. */
_Bool __torajs_process_stdout_write(const void *s) {
    const uint8_t *d = __TORAJS_STR_CDATA(s);
    uint64_t dlen = __TORAJS_STR_LEN(s);
    size_t put = fwrite(d, 1, (size_t)dlen, stdout);
    fflush(stdout);
    if (put != (size_t)dlen) {
        __torajs_panic("not yet supported: process.stdout.write short write");
    }
    return 1;
}

_Bool __torajs_process_stderr_write(const void *s) {
    const uint8_t *d = __TORAJS_STR_CDATA(s);
    uint64_t dlen = __TORAJS_STR_LEN(s);
    size_t put = fwrite(d, 1, (size_t)dlen, stderr);
    fflush(stderr);
    if (put != (size_t)dlen) {
        __torajs_panic("not yet supported: process.stderr.write short write");
    }
    return 1;
}

/* `process.stdin.read()` — bun's API is the Node.js Readable stream
 * shape: returns Buffer-or-null asynchronously, never a blocking
 * drain-to-EOF primitive. Implementing tr-side requires the async
 * substrate (v0.5 #2 async/await + #3 fetch). Deferred. The earlier
 * T-03 sketch synchronously drained to EOF and returned a Str — that
 * diverged from bun and was dropped to preserve tr-accepted parity. */

/* `a.flat()` — single-level array flattening. Outer array holds inner
 * array pointers (8 bytes each); we sum their lengths in pass 1, then
 * memcpy each into the result in pass 2. Element-type-agnostic.
 * v0 supports depth=1 only (no recursive flatten).
 */
void *__torajs_arr_flat(const uint8_t *outer) {
    uint64_t outer_len = __TORAJS_ARR_LEN(outer);
    uint64_t total = 0;
    for (uint64_t i = 0; i < outer_len; i++) {
        const uint8_t *inner = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(outer, i);
        total += __TORAJS_ARR_LEN(inner);
    }
    uint8_t *p = arr_alloc_(total, total);
    uint8_t *p_data = __TORAJS_ARR_DATA(p);  /* fresh, head=0 */
    uint64_t cursor = 0;
    for (uint64_t i = 0; i < outer_len; i++) {
        const uint8_t *inner = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(outer, i);
        uint64_t inner_len = __TORAJS_ARR_LEN(inner);
        if (inner_len) {
            memcpy(p_data + cursor,
                   __TORAJS_ARR_CSLOT(inner, 0),
                   (size_t)inner_len * 8);
            cursor += inner_len * 8;
        }
    }
    return p;
}

/* `a.concat(b)` — fresh array containing all of a's elements then all
 * of b's. Element-type-agnostic (8-byte slots). Single malloc + two
 * memcpys. Subset is two-arg only; JS allows `[...].concat(b, c, d)`
 * (multi-arg) but this v0 only handles the binary form. */
void *__torajs_arr_concat(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = __TORAJS_ARR_LEN(a);
    uint64_t b_len = __TORAJS_ARR_LEN(b);
    uint64_t total = a_len + b_len;
    uint8_t *p = arr_alloc_(total, total);
    uint8_t *p_data = __TORAJS_ARR_DATA(p);  /* fresh alloc, head=0 */
    if (a_len) memcpy(p_data, __TORAJS_ARR_CSLOT(a, 0), (size_t)a_len * 8);
    if (b_len) memcpy(p_data + (size_t)a_len * 8, __TORAJS_ARR_CSLOT(b, 0), (size_t)b_len * 8);
    return p;
}

/* `arr.reverse()` — in-place reverse over the i64-slot array. Returns
 * the same array pointer for chaining. Element-type-agnostic. */
void *__torajs_arr_reverse(uint8_t *arr) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    if (len < 2) return arr;
    uint64_t lo = 0, hi = len - 1;
    while (lo < hi) {
        uint64_t *a_slot = (uint64_t *)__TORAJS_ARR_SLOT(arr, lo);
        uint64_t *b_slot = (uint64_t *)__TORAJS_ARR_SLOT(arr, hi);
        uint64_t tmp = *a_slot;
        *a_slot = *b_slot;
        *b_slot = tmp;
        lo++; hi--;
    }
    return arr;
}

/* T-13.5 (v0.4.0) — `arr.shift()` is O(1): bump head_offset and
 * decrement len. No memmove. The vacated front slot stays allocated
 * until either compact (push hits cap-with-head>0) or the array's
 * drop releases the whole block.
 *
 * v0.6+1 perf checkpoint — body PROMOTED to inkwell IR (see
 * `define_arr_shift` in ssa_inkwell.rs). The C version is gone:
 * the C-side `__attribute__((always_inline))` doesn't survive the
 * native-object link boundary (inkwell emits .o not bitcode), so
 * `bl __torajs_arr_shift` stayed external in the linked binary
 * even with -flto. Defining the body directly in inkwell + the
 * IR-level `alwaysinline` attribute fixes that — LLVM splices the
 * 4-op body in before the .o ever forms.
 *
 * Symbol stays exported via inkwell so any (theoretical) cross-TU
 * caller still resolves. */

/* T-13.5 — `arr.unshift(v)` — insert `v` at slot[0]. Now O(1) when
 * head_offset > 0 (just decrement head, write into the freed
 * front slot). Falls back to compact-or-realloc-then-memmove when
 * head == 0 (true grow path). Returns the (possibly realloc'd)
 * array pointer, mirroring push's contract. */
void *__torajs_arr_unshift(uint8_t *arr, int64_t v) {
    uint32_t head = __TORAJS_ARR_HEAD(arr);
    if (head > 0) {
        /* Fast path: reclaim a freed front slot. */
        head -= 1;
        __TORAJS_ARR_HEAD(arr) = head;
        *(int64_t *)__TORAJS_ARR_DATA_RAW_SLOT(arr, head) = v;
        __TORAJS_ARR_LEN(arr) = __TORAJS_ARR_LEN(arr) + 1;
        return arr;
    }
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t cap = __TORAJS_ARR_CAP(arr);
    if (len >= cap) {
        /* Realloc — double cap (or 1 if 0). Live range moves to
         * physical slot 1; head stays 0 (caller's logical slot 0
         * is the new value at physical 0). */
        uint64_t new_cap = cap == 0 ? 1 : cap * 2;
        uint8_t *p = arr_alloc_(0, new_cap);
        if (len > 0) {
            memcpy(__TORAJS_ARR_DATA_RAW_SLOT(p, 1),
                   __TORAJS_ARR_DATA(arr),
                   (size_t)len * 8);
        }
        *(int64_t *)__TORAJS_ARR_DATA_RAW_SLOT(p, 0) = v;
        __TORAJS_ARR_LEN(p) = len + 1;
        free(arr);
        return p;
    }
    /* In-place: head==0 + cap room — memmove right and prepend. */
    if (len > 0) {
        memmove(__TORAJS_ARR_DATA_RAW_SLOT(arr, 1),
                __TORAJS_ARR_DATA(arr),
                (size_t)len * 8);
    }
    *(int64_t *)__TORAJS_ARR_DATA_RAW_SLOT(arr, 0) = v;
    __TORAJS_ARR_LEN(arr) = len + 1;
    return arr;
}

/* `arr.copyWithin(target, start, end)` — in-place memmove of
 * the [start, end) slice to position `target`. All indices clamped to
 * [0, len]. memmove handles overlap. Returns same pointer. */
void *__torajs_arr_copy_within(uint8_t *arr, int64_t target, int64_t start, int64_t end) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    int64_t lo = start < 0 ? 0 : (start > (int64_t)len ? (int64_t)len : start);
    int64_t hi = end < 0 ? 0 : (end > (int64_t)len ? (int64_t)len : end);
    int64_t to = target < 0 ? 0 : (target > (int64_t)len ? (int64_t)len : target);
    if (hi <= lo) return arr;
    int64_t count = hi - lo;
    if (to + count > (int64_t)len) {
        count = (int64_t)len - to;
        if (count <= 0) return arr;
    }
    memmove(__TORAJS_ARR_SLOT(arr, to),
            __TORAJS_ARR_SLOT(arr, lo),
            (size_t)count * 8);
    return arr;
}

/* `arr.fill(value, start, end)` — write `value` into [start, end).
 * Both indices clamped to [0, len]. Element-type-agnostic — the value
 * is passed as i64 and stored verbatim in each slot; the caller's
 * SSA layer is responsible for converting types. Returns the same
 * pointer for chaining. */
void *__torajs_arr_fill(uint8_t *arr, int64_t value, int64_t start, int64_t end) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    int64_t lo = start < 0 ? 0 : (start > (int64_t)len ? (int64_t)len : start);
    int64_t hi = end < 0 ? 0 : (end > (int64_t)len ? (int64_t)len : end);
    if (hi < lo) return arr;
    for (int64_t i = lo; i < hi; i++) {
        *(int64_t *)__TORAJS_ARR_SLOT(arr, i) = value;
    }
    return arr;
}

/* __torajs_str_to_upper / __torajs_str_to_lower moved to
 * torajs-str::transform::case (P3.1-e.1, 2026-05-23). ASCII-only
 * fold preserved bit-for-bit; non-ASCII bytes pass through. IR-side
 * intrinsic declarations in ssa_lower (and the alloc-noalias
 * whitelist in ssa_inkwell::is_alloc_intrinsic) resolve to the
 * Rust extern "C" wrappers in the libtorajs_str.a staticlib link. */

#include <math.h>

void *__torajs_num_to_string_radix_i(int64_t n, int64_t radix);

/* V3-18 wedge — `n.toString(radix)` for f64 receivers. Mirrors the
 * spec algorithm in §21.1.3.6 / §6.1.6.1.13: encode the integer
 * part in the given radix, then loop the fractional part multiplying
 * by radix until either the fraction becomes 0 or we hit a digit
 * cap. Mantissa precision is 52 bits, so log_radix(2^52) digits is
 * the upper bound: 52 for radix 2, ~13 for radix 16, ~32 for
 * radix 4. We cap at 52 (the binary case) — anything past that is
 * rounding noise. NaN / Infinity / -Infinity preserve the canonical
 * formatter outputs. */
void *__torajs_num_to_string_radix_f(double d, int64_t radix) {
    if (radix < 2) radix = 2;
    if (radix > 36) radix = 36;
    if (d != d) {
        uint8_t *p = __torajs_str_alloc_pooled(3);
        memcpy(__TORAJS_STR_DATA(p), "NaN", 3);
        return p;
    }
    if (d == 1.0 / 0.0) {
        uint8_t *p = __torajs_str_alloc_pooled(8);
        memcpy(__TORAJS_STR_DATA(p), "Infinity", 8);
        return p;
    }
    if (d == -1.0 / 0.0) {
        uint8_t *p = __torajs_str_alloc_pooled(9);
        memcpy(__TORAJS_STR_DATA(p), "-Infinity", 9);
        return p;
    }
    /* Integer-valued: route to the integer path. The (int64_t) cast
     * is safe because the integer-valued check pinned d to a finite
     * representable integer. */
    if (d == floor(d) && d >= (double)INT64_MIN && d <= (double)INT64_MAX) {
        return __torajs_num_to_string_radix_i((int64_t)d, radix);
    }
    static const char digits[] = "0123456789abcdefghijklmnopqrstuvwxyz";
    int neg = d < 0;
    double abs_d = neg ? -d : d;
    double int_part = floor(abs_d);
    double frac = abs_d - int_part;
    /* Integer-part digits — use the existing path, drop the
     * sign because we'll prepend our own. */
    void *int_str = __torajs_num_to_string_radix_i((int64_t)int_part, radix);
    uint64_t int_len = __TORAJS_STR_LEN(int_str);
    const uint8_t *int_bytes = __TORAJS_STR_CDATA(int_str);
    /* Fractional digits — multiply / extract / subtract loop. Cap
     * at 52 (worst-case radix 2 mantissa bits). */
    char frac_buf[64];
    int frac_n = 0;
    double r_d = (double)radix;
    while (frac > 0.0 && frac_n < 52) {
        frac *= r_d;
        double digit_d = floor(frac);
        int digit = (int)digit_d;
        if (digit < 0) digit = 0;
        if (digit >= (int)radix) digit = (int)radix - 1;
        frac_buf[frac_n++] = digits[digit];
        frac -= digit_d;
    }
    /* Build "[-]<int>.<frac>" (no fractional dot if frac is empty). */
    uint64_t total_len = int_len + (neg ? 1 : 0)
                       + (frac_n > 0 ? 1 + (uint64_t)frac_n : 0);
    uint8_t *p = __torajs_str_alloc_pooled(total_len);
    uint8_t *out = __TORAJS_STR_DATA(p);
    uint64_t off = 0;
    if (neg) out[off++] = '-';
    memcpy(out + off, int_bytes, int_len);
    off += int_len;
    if (frac_n > 0) {
        out[off++] = '.';
        memcpy(out + off, frac_buf, (size_t)frac_n);
        off += (uint64_t)frac_n;
    }
    /* int_str was a fresh alloc only for the digits — drop it so
     * its refcount goes to 0 and the heap is released. */
    __torajs_str_drop(int_str);
    return p;
}

/* `n.toString(radix)` for integers — encode i64 value in the given
 * radix (2..36). Negative numbers get a leading `-`. */
void *__torajs_num_to_string_radix_i(int64_t n, int64_t radix) {
    if (radix < 2) radix = 2;
    if (radix > 36) radix = 36;
    char buf[80];
    static const char digits[] = "0123456789abcdefghijklmnopqrstuvwxyz";
    int neg = n < 0;
    uint64_t u;
    if (neg) {
        // Two's complement abs handles INT64_MIN by overflow-wrap.
        u = (uint64_t)(-(n + 1)) + 1;
    } else {
        u = (uint64_t)n;
    }
    int i = (int)sizeof(buf);
    if (u == 0) {
        buf[--i] = '0';
    } else {
        while (u > 0) {
            buf[--i] = digits[u % (uint64_t)radix];
            u /= (uint64_t)radix;
        }
    }
    if (neg) buf[--i] = '-';
    int len = (int)sizeof(buf) - i;
    uint8_t *p = __torajs_str_alloc_pooled((uint64_t)len);
    if (len) memcpy(__TORAJS_STR_DATA(p), &buf[i], (size_t)len);
    return p;
}

/* `n.toFixed(digits)` — fixed-point decimal as a fresh String. JS spec
 * accepts 0..100 digits; subset clamps to 0..20. snprintf gives spec-
 * matching round-half-to-even on most libcs (close enough for the
 * common cases). */
void *__torajs_num_to_fixed_f(double n, int64_t digits) {
    if (digits < 0) digits = 0;
    if (digits > 20) digits = 20;
    /* V3-18 wedge — match JS spec §21.1.3.4: "ties broken by
     * choosing the larger m", i.e. round half AWAY from zero.
     * snprintf's default (round-half-to-even on macOS libc)
     * diverges for .5 cases like 1234.5.toFixed(0). Pre-multiply,
     * round() (half-away-from-zero per C99 7.12.9.6), divide back,
     * then format with %.*f for the trailing-zero padding. */
    if (isfinite(n) && digits < 16) {
        double scale = 1.0;
        for (int64_t i = 0; i < digits; i++) scale *= 10.0;
        n = round(n * scale) / scale;
    }
    char buf[64];
    int written = snprintf(buf, sizeof(buf), "%.*f", (int)digits, n);
    if (written < 0) written = 0;
    uint64_t len = (uint64_t)written;
    uint8_t *p = __torajs_str_alloc_pooled(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), buf, (size_t)len);
    return p;
}
void *__torajs_num_to_fixed_i(int64_t n, int64_t digits) {
    return __torajs_num_to_fixed_f((double)n, digits);
}

/* Strip leading zeros from an exponent in `<...>e<sign><digits>` so
 * `1.23e+03` becomes `1.23e+3`, matching JS spec. Returns the new
 * length. */
static int js_normalize_exp_(const char *src, int src_len, char *dst) {
    int dst_i = 0;
    int i = 0;
    while (i < src_len) {
        char c = src[i++];
        dst[dst_i++] = c;
        if (c == 'e' && i < src_len) {
            char sign = src[i];
            if (sign == '+' || sign == '-') {
                dst[dst_i++] = sign;
                i++;
            }
            while (i < src_len && src[i] == '0') i++;
            if (i >= src_len || src[i] < '0' || src[i] > '9') {
                dst[dst_i++] = '0';
            }
        }
    }
    return dst_i;
}

/* `n.toExponential(digits)` — scientific form. snprintf %.*e with the
 * given precision, then strip leading zeros from the exponent. */
void *__torajs_num_to_exp_f(double n, int64_t digits) {
    if (digits < 0) digits = 0;
    if (digits > 100) digits = 100;
    char buf[128];
    int written = snprintf(buf, sizeof(buf), "%.*e", (int)digits, n);
    if (written < 0) written = 0;
    char fixed[128];
    int dst_len = js_normalize_exp_(buf, written, fixed);
    uint64_t len = (uint64_t)dst_len;
    uint8_t *p = __torajs_str_alloc_pooled(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), fixed, (size_t)len);
    return p;
}
void *__torajs_num_to_exp_i(int64_t n, int64_t digits) {
    return __torajs_num_to_exp_f((double)n, digits);
}

/* `n.toPrecision(digits)` — total significant digits. snprintf %.*g
 * with exponent normalization. digits == 0 falls back to default %g. */
void *__torajs_num_to_precision_f(double n, int64_t digits) {
    char buf[128];
    int written;
    if (digits <= 0) {
        written = snprintf(buf, sizeof(buf), "%g", n);
    } else {
        if (digits > 100) digits = 100;
        written = snprintf(buf, sizeof(buf), "%.*g", (int)digits, n);
    }
    if (written < 0) written = 0;
    char fixed[128];
    int dst_len = js_normalize_exp_(buf, written, fixed);
    uint64_t len = (uint64_t)dst_len;
    uint8_t *p = __torajs_str_alloc_pooled(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), fixed, (size_t)len);
    return p;
}
void *__torajs_num_to_precision_i(int64_t n, int64_t digits) {
    return __torajs_num_to_precision_f((double)n, digits);
}

/* `Number.parseInt(s, radix)` — JS-spec parseInt, simplified subset.
 * Skips leading ASCII whitespace, accepts optional sign, then digits in
 * the given radix (2..36). Stops at the first non-digit. Returns NaN
 * encoded as the IEEE-754 quiet-NaN bit pattern when no digits are
 * consumed; otherwise the parsed double. radix=0 → autodetect (10
 * default; 16 if "0x"/"0X" prefix). */
double __torajs_num_parse_int(const uint8_t *s, int64_t radix) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *data = __TORAJS_STR_CDATA(s);
    uint64_t i = 0;
    while (i < len && (data[i] == ' ' || data[i] == '\t' || data[i] == '\n'
                       || data[i] == '\r' || data[i] == '\v' || data[i] == '\f')) {
        i++;
    }
    int sign = 1;
    if (i < len && (data[i] == '+' || data[i] == '-')) {
        if (data[i] == '-') sign = -1;
        i++;
    }
    int rdx = (int)radix;
    if (rdx == 0) rdx = 10;
    /* 0x / 0X auto-radix when caller passed 0 or 16. */
    if ((radix == 0 || radix == 16) && i + 1 < len
        && data[i] == '0' && (data[i + 1] == 'x' || data[i + 1] == 'X')) {
        rdx = 16;
        i += 2;
    }
    if (rdx < 2 || rdx > 36) return (double)NAN;
    uint64_t digits_start = i;
    double v = 0.0;
    while (i < len) {
        uint8_t c = data[i];
        int d;
        if (c >= '0' && c <= '9') d = c - '0';
        else if (c >= 'a' && c <= 'z') d = c - 'a' + 10;
        else if (c >= 'A' && c <= 'Z') d = c - 'A' + 10;
        else break;
        if (d >= rdx) break;
        v = v * rdx + d;
        i++;
    }
    if (i == digits_start) return (double)NAN;
    return sign < 0 ? -v : v;
}

/* `Number.parseFloat(s)` — strtod over the trimmed prefix. Stops at
 * the first non-numeric byte. Returns NaN if no digits parsed. */
double __torajs_num_parse_float(const uint8_t *s) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *data = __TORAJS_STR_CDATA(s);
    /* Copy into a NUL-terminated buffer so strtod's bounds work. JS-allowed
     * input shapes (sign, digits, exponent, +/-Infinity) all fit within
     * len + 1 bytes; for very long inputs we'd need malloc — out of scope. */
    char buf[64];
    uint64_t copy = len < sizeof(buf) - 1 ? len : sizeof(buf) - 1;
    memcpy(buf, data, (size_t)copy);
    buf[copy] = 0;
    char *endp = NULL;
    double v = strtod(buf, &endp);
    if (endp == buf) return (double)NAN;
    return v;
}

/* `Number.isSafeInteger(n)` — true iff n is an integer-valued number
 * within [-(2^53 - 1), 2^53 - 1]. Safe means a round-trip through f64
 * preserves the value exactly. */
int64_t __torajs_num_is_safe_integer_f(double n) {
    if (!isfinite(n)) return 0;
    if (floor(n) != n) return 0;
    double max_safe = 9007199254740991.0; /* 2^53 - 1 */
    return (n >= -max_safe && n <= max_safe) ? 1 : 0;
}
int64_t __torajs_num_is_safe_integer_i(int64_t n) {
    int64_t max_safe = 9007199254740991;
    return (n >= -max_safe && n <= max_safe) ? 1 : 0;
}

/* `Number.isInteger(n)` — true iff n is finite and has no fractional
 * part. ECMA-262 §20.1.2.3. */
int64_t __torajs_num_is_integer_f(double n) {
    if (!isfinite(n)) return 0;
    return floor(n) == n ? 1 : 0;
}
int64_t __torajs_num_is_integer_i(int64_t n) {
    (void)n;
    return 1;
}

/* `Number.isNaN(n)` — true iff n is NaN. (Distinct from global `isNaN`
 * which coerces non-numbers; the Number.isX form does not coerce.) */
int64_t __torajs_num_is_nan_f(double n) {
    return isnan(n) ? 1 : 0;
}
int64_t __torajs_num_is_nan_i(int64_t n) {
    (void)n;
    return 0;
}

/* `Number.isFinite(n)` — true iff n is a finite number. */
int64_t __torajs_num_is_finite_f(double n) {
    return isfinite(n) ? 1 : 0;
}
int64_t __torajs_num_is_finite_i(int64_t n) {
    (void)n;
    return 1;
}

/* Whitespace recognition for `trim*`: ASCII whitespace ' ', '\t', '\n',
 * '\r', '\v', '\f'. JS spec includes more (BOM, NBSP, …) but those are
 * UTF-16 units we don't model in v0. */
/* __torajs_str_trim / _trim_start / _trim_end + the static is_trim_ws_
 * predicate moved to torajs-str::transform::trim (P3.1-e.2, 2026-05-23).
 * ASCII whitespace set preserved bit-for-bit (space/tab/LF/CR/VT/FF).
 * IR-side intrinsic declarations in ssa_lower + alloc-noalias whitelist
 * in ssa_inkwell resolve via the libtorajs_str.a staticlib link. */

/* __torajs_str_pad_start / _pad_end moved to torajs-str::transform::pad
 * (P3.1-e.3, 2026-05-23). Byte-length semantics preserved. Empty pad
 * → space fill (matches C subset). IR-side intrinsic declarations in
 * ssa_lower + alloc-noalias whitelist in ssa_inkwell resolve via
 * libtorajs_str.a staticlib link. */

/* M6.3 — JSON.parse runtime helpers. Cursor is `int64_t *pos`,
 * updated in place by every helper so ssa_lower's compile-time
 * specialized parser can thread one alloca'd slot through all
 * recursive calls. On syntactic mismatch each helper stuffs an
 * error string into the throw_active / throw_value globals via
 * `__torajs_throw_set` and returns a default; ssa_lower emits a
 * `throw_check` after each call so propagation flows correctly.
 */

extern void __torajs_throw_set(int64_t tag, int64_t value);

static void torajs_json_throw(const char *msg, int64_t pos) {
    char buf[96];
    int n = snprintf(buf, sizeof(buf), "%s at pos %lld", msg, (long long)pos);
    if (n < 0) n = 0;
    if ((size_t)n >= sizeof(buf)) n = (int)sizeof(buf) - 1;
    uint64_t len = (uint64_t)n;
    uint8_t *err = __torajs_str_alloc_pooled(len);
    if (len) memcpy(__TORAJS_STR_DATA(err), buf, (size_t)len);
    __torajs_throw_set(4 /* ANY_HEAP */, (int64_t)(uintptr_t)err);
}

static void torajs_json_skip_ws(const uint8_t *data, uint64_t len, int64_t *pos) {
    while (*pos < (int64_t)len) {
        uint8_t c = data[*pos];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
            (*pos)++;
        } else {
            break;
        }
    }
}

void __torajs_json_eat_char(const uint8_t *str, int64_t *pos, int64_t want) {
    uint64_t len = __TORAJS_STR_LEN(str);
    const uint8_t *data = __TORAJS_STR_CDATA(str);
    torajs_json_skip_ws(data, len, pos);
    if (*pos >= (int64_t)len || data[*pos] != (uint8_t)want) {
        char m[40];
        snprintf(m, sizeof(m), "JSON.parse: expected '%c'", (char)want);
        torajs_json_throw(m, *pos);
        return;
    }
    (*pos)++;
}

int64_t __torajs_json_parse_int(const uint8_t *str, int64_t *pos) {
    uint64_t len = __TORAJS_STR_LEN(str);
    const uint8_t *data = __TORAJS_STR_CDATA(str);
    torajs_json_skip_ws(data, len, pos);
    int64_t start = *pos;
    int64_t neg = 0;
    if (*pos < (int64_t)len && data[*pos] == '-') {
        neg = 1;
        (*pos)++;
    }
    int64_t digits_start = *pos;
    int64_t value = 0;
    while (*pos < (int64_t)len && data[*pos] >= '0' && data[*pos] <= '9') {
        value = value * 10 + (int64_t)(data[*pos] - '0');
        (*pos)++;
    }
    if (*pos == digits_start) {
        torajs_json_throw("JSON.parse: expected number digits", start);
        return 0;
    }
    return neg ? -value : value;
}

double __torajs_json_parse_float(const uint8_t *str, int64_t *pos) {
    uint64_t len = __TORAJS_STR_LEN(str);
    const uint8_t *data = __TORAJS_STR_CDATA(str);
    torajs_json_skip_ws(data, len, pos);
    int64_t start = *pos;
    int64_t end = start;
    if (end < (int64_t)len && data[end] == '-') end++;
    while (end < (int64_t)len && data[end] >= '0' && data[end] <= '9') end++;
    if (end < (int64_t)len && data[end] == '.') {
        end++;
        while (end < (int64_t)len && data[end] >= '0' && data[end] <= '9') end++;
    }
    if (end < (int64_t)len && (data[end] == 'e' || data[end] == 'E')) {
        end++;
        if (end < (int64_t)len && (data[end] == '+' || data[end] == '-')) end++;
        while (end < (int64_t)len && data[end] >= '0' && data[end] <= '9') end++;
    }
    if (end == start || (end == start + 1 && data[start] == '-')) {
        torajs_json_throw("JSON.parse: expected number digits", start);
        return 0.0;
    }
    char buf[64];
    uint64_t blen = (uint64_t)(end - start);
    if (blen >= sizeof(buf)) blen = sizeof(buf) - 1;
    memcpy(buf, data + start, blen);
    buf[blen] = '\0';
    *pos = end;
    return strtod(buf, NULL);
}

int64_t __torajs_json_parse_bool(const uint8_t *str, int64_t *pos) {
    uint64_t len = __TORAJS_STR_LEN(str);
    const uint8_t *data = __TORAJS_STR_CDATA(str);
    torajs_json_skip_ws(data, len, pos);
    int64_t start = *pos;
    if (*pos + 4 <= (int64_t)len && memcmp(data + *pos, "true", 4) == 0) {
        *pos += 4;
        return 1;
    }
    if (*pos + 5 <= (int64_t)len && memcmp(data + *pos, "false", 5) == 0) {
        *pos += 5;
        return 0;
    }
    torajs_json_throw("JSON.parse: expected 'true' or 'false'", start);
    return 0;
}

void *__torajs_json_parse_string(const uint8_t *str, int64_t *pos) {
    uint64_t len = __TORAJS_STR_LEN(str);
    const uint8_t *data = __TORAJS_STR_CDATA(str);
    torajs_json_skip_ws(data, len, pos);
    int64_t start = *pos;
    if (*pos >= (int64_t)len || data[*pos] != '"') {
        torajs_json_throw("JSON.parse: expected string", start);
        return __torajs_str_alloc_pooled(0);
    }
    (*pos)++;
    /* Pass 1: scan to find the closing quote + count decoded length. */
    uint64_t out_len = 0;
    int64_t scan = *pos;
    while (scan < (int64_t)len) {
        uint8_t c = data[scan];
        if (c == '"') break;
        if (c == '\\') {
            if (scan + 1 >= (int64_t)len) {
                torajs_json_throw("JSON.parse: bad escape", scan);
                return __torajs_str_alloc_pooled(0);
            }
            uint8_t e = data[scan + 1];
            if (e == 'u') {
                if (scan + 6 > (int64_t)len) {
                    torajs_json_throw("JSON.parse: short \\u escape", scan);
                    return __torajs_str_alloc_pooled(0);
                }
                out_len += 1;
                scan += 6;
            } else {
                out_len += 1;
                scan += 2;
            }
            continue;
        }
        out_len += 1;
        scan += 1;
    }
    if (scan >= (int64_t)len) {
        torajs_json_throw("JSON.parse: unterminated string", start);
        return __torajs_str_alloc_pooled(0);
    }
    /* Pass 2: write decoded bytes. */
    uint8_t *p = __torajs_str_alloc_pooled(out_len);
    uint8_t *out = __TORAJS_STR_DATA(p);
    uint64_t j = 0;
    int64_t i = *pos;
    while (i < scan) {
        uint8_t c = data[i];
        if (c == '\\') {
            uint8_t e = data[i + 1];
            switch (e) {
                case '"':  out[j++] = '"';  i += 2; break;
                case '\\': out[j++] = '\\'; i += 2; break;
                case '/':  out[j++] = '/';  i += 2; break;
                case 'b':  out[j++] = '\b'; i += 2; break;
                case 'f':  out[j++] = '\f'; i += 2; break;
                case 'n':  out[j++] = '\n'; i += 2; break;
                case 'r':  out[j++] = '\r'; i += 2; break;
                case 't':  out[j++] = '\t'; i += 2; break;
                case 'u': {
                    int v = 0;
                    for (int k = 0; k < 4; k++) {
                        uint8_t h = data[i + 2 + k];
                        v <<= 4;
                        if (h >= '0' && h <= '9') v |= (h - '0');
                        else if (h >= 'a' && h <= 'f') v |= (h - 'a' + 10);
                        else if (h >= 'A' && h <= 'F') v |= (h - 'A' + 10);
                    }
                    out[j++] = (uint8_t)(v & 0xFF);
                    i += 6;
                    break;
                }
                default:
                    out[j++] = e;
                    i += 2;
                    break;
            }
        } else {
            out[j++] = c;
            i += 1;
        }
    }
    *pos = scan + 1; /* skip closing quote */
    return p;
}

/* Returns 1 if the next token is a continuation comma (consumed),
 * 0 if the next token is the terminator (consumed), -1 only if a
 * syntactic error was thrown. The terminator byte is the caller's
 * responsibility — `]` for arrays, `}` for objects. */
int64_t __torajs_json_arr_step(const uint8_t *str, int64_t *pos, int64_t terminator) {
    uint64_t len = __TORAJS_STR_LEN(str);
    const uint8_t *data = __TORAJS_STR_CDATA(str);
    torajs_json_skip_ws(data, len, pos);
    if (*pos >= (int64_t)len) {
        torajs_json_throw("JSON.parse: unexpected end-of-input", *pos);
        return -1;
    }
    uint8_t c = data[*pos];
    if (c == ',') { (*pos)++; return 1; }
    if (c == (uint8_t)terminator) { (*pos)++; return 0; }
    char m[64];
    snprintf(m, sizeof(m), "JSON.parse: expected ',' or '%c'", (char)terminator);
    torajs_json_throw(m, *pos);
    return -1;
}

/* Peek at the first token after an opening bracket. Returns 0 if the
 * immediate next non-ws byte is the terminator (consumed; empty
 * collection), 1 if the next byte begins a value (NOT consumed), -1
 * on EOF / error. Lets the array / object parsers skip the leading-
 * element parse on `[]` / `{}`. */
int64_t __torajs_json_arr_first(const uint8_t *str, int64_t *pos, int64_t terminator) {
    uint64_t len = __TORAJS_STR_LEN(str);
    const uint8_t *data = __TORAJS_STR_CDATA(str);
    torajs_json_skip_ws(data, len, pos);
    if (*pos >= (int64_t)len) {
        torajs_json_throw("JSON.parse: unexpected end-of-input", *pos);
        return -1;
    }
    if (data[*pos] == (uint8_t)terminator) {
        (*pos)++;
        return 0;
    }
    return 1;
}

/* __torajs_str_eq_cstr moved to torajs-str::eq (P3.1-c, 2026-05-23).
 * Used by the object parser to verify a parsed key against an
 * expected field name. Rust impl shares the same `bytes_eq` core
 * as __torajs_str_eq above. */
