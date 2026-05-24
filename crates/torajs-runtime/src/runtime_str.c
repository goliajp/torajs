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
/* Split-block pool state + split_block_alloc_ moved to
 * torajs-str::split::pool (P3.1-f, 2026-05-23). __torajs_arr_free's
 * SPLIT_BLOCK branch now calls the Rust extern
 * __torajs_split_block_free_push instead of pushing inline. */
extern int __torajs_split_block_free_push(void *p);

/* arr_pool LIFO state (arr_pool_blocks_/caps_/count_) +
 * __torajs_arr_free + __torajs_arr_alloc_pooled 全部 moved to
 * torajs-arr::{pool, alloc} (P4.1-b, 2026-05-23). POOL_SLOTS=16,
 * POOL_CAP_MAX=32 mirror C 常量. AtomicPtr/AtomicU64/AtomicUsize 在
 * 单线程 runtime 下用 Ordering::Relaxed compile 出来跟 static mut
 * 同一指令. SPLIT_BLOCK cross-tier 走 __torajs_split_block_free_push
 * (torajs-str::split::pool). */

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

/* any_slot_tag_/val_ helpers + __TORAJS_ANY_SLOT_BYTES macro deleted
 * (P4.1-e, 2026-05-23). Last C user was __torajs_arr_drop_any which
 * now lives in torajs-arr::drop. ANY_SLOT_BYTES=16 const mirror is
 * in torajs-arr/src/drop.rs + any.rs as `ANY_SLOT_BYTES`. */

/* __torajs_arr_alloc_any / _alloc_any_filled / _push_any / _extend_any
 * / _get_any_tag / _get_any_value / _set_any 全部 moved to
 * torajs-arr::any (P4.1-d, 2026-05-23). 16-byte tagged slot stride
 * (vs 8 for Array<T>), FLAG_ARR_ANY 标记 routes free 出 cap-matched
 * pool (不同 stride). extend_any / set_any 含 rc_inc / value_drop_heap
 * cross-tier 调用 for ANY_HEAP slot refcount 平衡. any_slot_tag_/val_
 * helpers 跟着搬到 Rust. ANY_UNDEF=5 / ANY_HEAP=4 mirrored. */

/* Forward decl — definition lives further down in the file (was used
 * by __torajs_arr_set_any, now in torajs-arr::any via cross-tier extern). */
void __torajs_value_drop_heap(void *child);

/* P3.1 — Dynamic-property object substrate (HashMap-backed).
 *
 * The complete impl moved to the `torajs-dynobj` crate over phase
 * P4.2 (a..e, 2026-05-23). C-side keeps only the cross-tier `extern`
 * decls for the symbols still referenced from this file
 * (getOwnPropertyDescriptor's wrapper at ~line 622 and node-lookup
 * around ~line 940 both call alloc/has/get_*; value_drop_heap dispatch
 * calls drop). The original layout / macros / typedef / probe / hash /
 * str_eq / set / define / resize / get / has / delete / drop bodies are
 * all gone — see git history (`git log -- crates/torajs-runtime/src/runtime_str.c`)
 * for the pre-port C source.
 *
 * Layout (for reference only; canonical defs live in
 * `crates/torajs-dynobj/src/layout.rs`):
 *   offset 0  : heap header (8)
 *   offset 8  : count (u32) / cap (u32) / tomb (u32) / pad (u32)
 *   offset 24 : buckets[cap] — 24B each ({ key_ptr:*Str, tag:u64, value:u64 }) */

extern void *__torajs_dynobj_alloc(void);
extern uint64_t __torajs_dynobj_get_tag(const void *obj, const void *key);
extern uint64_t __torajs_dynobj_get_value(const void *obj, const void *key);
extern uint64_t __torajs_dynobj_get_flags(const void *obj, const void *key);
extern void __torajs_dynobj_set(void **obj_slot, void *key, uint64_t tag, uint64_t value);
extern void __torajs_dynobj_define(
    void **obj_slot,
    void *key,
    uint64_t tag,
    uint64_t value,
    uint64_t flags_byte
);
extern int __torajs_dynobj_has(const void *obj, const void *key);
extern int __torajs_dynobj_delete(void *obj, const void *key);
extern void __torajs_dynobj_drop(void *obj);

/* Get tag for `key`. Returns ANY_UNDEF=5 when the key isn't present
 * (per ES spec — missing property reads as undefined). Also returns
 * ANY_UNDEF when `obj` is not a dynobj (e.g. a typed Struct passed
 * via Any-box from `obj?.field.subfield` chained access). Without
 * this defensive check, dynobj_probe would index into the wrong
 * layout and return garbage tag values silently. */
/* __torajs_dynobj_get_tag / get_value / get_flags moved to
 * torajs-dynobj::get (P4.2-b, 2026-05-23). Extern decls + note in
 * the forward-decl block earlier in this section. */

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
extern void __torajs_value_drop_heap(void *child);
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

/* __torajs_dynobj_set / define / has / delete / drop all moved to the
 * `torajs-dynobj` crate (P4.2-c..e, 2026-05-23). Extern decls are
 * grouped at the top of the dynobj section (~line 446). */

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

/* __torajs_arr_drop_any moved to torajs-arr::drop (P4.1-e, 2026-05-23).
 * Rust impl mirrors C 1:1: NULL/STATIC_LITERAL/rc_dec gates +
 * per-slot heap-child walker (ANY_HEAP tag → value_drop_heap) +
 * arrprops_drop_entry + libc free. Bypasses pool (16-byte stride). */
void __torajs_arrprops_drop_entry(void *arr_ptr); /* fwd decl — impl
                                                    now in torajs-arr::props
                                                    (P4.1-i); resolved at
                                                    link time via libtorajs_arr.a */

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

/* T-29 — Array-as-Object side table for `arr.x = v` / `arr.x` reads.
 *
 * 4 extern fns (__torajs_arrprops_{set,get_tag,get_value,drop_entry}) +
 * 3 static helpers (find / intern / hash) moved to torajs-arr::props
 * (P4.1-i, 2026-05-23). Pure C→Rust port: same 256-bucket pointer-
 * keyed hashtable, same MurmurHash finalizer, same lazy dynobj alloc.
 * Drop hook (called from arr_drop / arr_drop_any on rc→0) still works
 * the same way; the dynobj is owned 1:1 by the entry and gets
 * value_drop_heap'd on removal.
 *
 * dynobj alloc/set/get_tag/get_value + value_drop_heap remain in this
 * file (C-side) for now — they get ported in P4.2 (torajs-dynobj sub-
 * crate). Rust side calls them via extern "C". */

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

/* Substr method helpers (char_code_at / eq_str / to_owned /
 * concat_substr_str / concat_str_substr / concat_substr_substr /
 * starts_with / ends_with / includes / index_of / slice /
 * substring / trim / trim_start / trim_end) moved to pure-Rust
 * `torajs-str::substr_methods` at P7.a (2026-05-24). Resolved at
 * `tr build` link time via libtorajs_str.a. */

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

/* __torajs_arr_extend_unchecked moved to torajs-arr::ops (P4.1-c,
 * 2026-05-23). Pure-Rust memcpy + len bump; T-13.5 head_offset folded
 * into source/dest slot pointers. */

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
/* __torajs_math_round moved to torajs-num::math (P3.2-b, 2026-05-23).
 * JS spec semantics preserved bit-for-bit: (x + 0.5).floor() — NOT
 * libc round which uses away-from-zero on negative halves. */

/* __torajs_str_repeat moved to torajs-str::transform::construct
 * (P3.1-e.4, 2026-05-23). n<=0 clamps to 0; wrapping_mul matches
 * the C subset's silent-overflow contract. */

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

/* __torajs_arr_slice moved to torajs-arr::slice (P4.1-f, 2026-05-23).
 * ES §22.1.3.25 negative-index clamp + single malloc + memcpy preserved
 * 1:1. T-13.5 head_offset folded into source pointer. */

/* __torajs_i64_to_str / __torajs_f64_to_str / __torajs_bool_to_str /
 * __torajs_print_f64_js (+ torajs_f64_shortest helper) moved to
 * torajs-num::to_str (P7.e, 2026-05-24). __torajs_null_to_str /
 * __torajs_undefined_to_str moved to torajs-str::literals (same
 * commit). All preserve the libc snprintf("%.0f") / snprintf("%.*g")
 * + strtod round-trip path bit-for-bit. */
#define __TORAJS_ARR_DATA_OFF 24

/* __torajs_arr_print_{i64,f64,bool,str} moved to torajs-arr::print
 * (P4.1-g, 2026-05-23). Same per-element shape, same byte-equal output
 * via cross-tier putchar + snprintf. NaN/Infinity/-Infinity 特殊 case
 * for f64 preserved. */

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

/* __torajs_arr_print_substr moved to torajs-arr::print (P4.1-g,
 * 2026-05-23). Substr layout-aware: reads parent + offset + len from
 * each slot's Substr header, writes "<bytes>" with quotes. */

/* JS-spec f64 console formatter (__torajs_print_f64_js) + f64_to_str
 * + bool/null/undefined → Str helpers + str_to_number all moved to
 * Rust sub-crates (torajs-num::to_str + torajs-str::literals +
 * torajs-str::to_number). See the move comment above _i64_to_str. */


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


/* `__torajs_arr_push_unchecked` is inkwell-defined and exported as a
 * regular extern symbol; declare it here so the C runtime can call it
 * from split's pre-sized fast path (skips per-push capacity check +
 * potential realloc). */
void __torajs_arr_push_unchecked(void *arr, int64_t val);

/* __torajs_str_split + __torajs_split_init_inline moved to
 * torajs-str::split::ops (P3.1-f, 2026-05-23). Single-block layout
 * [arr_hdr:24][N*8 ptr slots][N*32 inline substr] preserved bit-
 * for-bit. Pool fast-path delegated to torajs-str::split::pool;
 * IR-side intrinsic declaration in ssa_lower and alloc-noalias
 * whitelist in ssa_inkwell unchanged. */

/* SplitIter (struct + init + drop) moved to torajs-str::split::ops
 * (P3.1-f, 2026-05-23). 48-byte layout preserved bit-for-bit so
 * the IR-emitted __torajs_split_iter_next (ssa_inkwell::
 * define_split_iter_next) keeps resolving fields by hardcoded
 * offset. _next body still lives in IR; consolidates to Rust in
 * P3.1-g. */

/* __torajs_arr_join (Array<Str>) moved to torajs-arr::join (P4.1-h,
 * 2026-05-23). Two-pass sum+memcpy preserved 1:1. Str output via
 * cross-tier __torajs_str_alloc_pooled. */

/* __torajs_arr_join_{i64,f64,bool,substr} 全部 moved to torajs-arr::join
 * (P4.1-h, 2026-05-23). 同 join_str 形态: two-pass sum+memcpy. f64 用
 * Rust 端 f64_shortest helper (snprintf+strtod loop, 同 C 1:1). */

/* __torajs_str_from_char_code moved to torajs-str::transform::construct
 * (P3.1-e.4, 2026-05-23). 1-byte Str holding `n & 0xff` — matches
 * v0 byte-Str layout (no UTF-8 encoding for non-ASCII). */

/* __torajs_arr_to_reversed + _with moved to torajs-arr::join
 * (P4.1-h, 2026-05-23). ES2023 non-mutating: single malloc + element-
 * wise slot copy, original untouched. */

/* __torajs_str_substring moved to torajs-str::transform::construct
 * (P3.1-e.4, 2026-05-23). Negative inputs clamp to 0 (no wrap);
 * start > end is silently swapped before slicing. */

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

/* __torajs_arr_set_length_validate moved to torajs-arr::grow (P4.1-j,
 * 2026-05-23). Rust port — same ES §9.4.2.4 round-trip check, same
 * tag dispatch. Symbol resolves cross-tier at `tr build` link time
 * via libtorajs_arr.a. */

/* __torajs_str_substr moved to torajs-str::transform::construct
 * (P3.1-e.4, 2026-05-23). AnnexB legacy: negative start wraps to
 * max(size+start, 0); length clamps to remaining bytes. */

/* __torajs_arr_from_string moved to torajs-arr::from_string
 * (P7.e, 2026-05-24). Pre-sizes the array cap to `s.len` so per-
 * element push is O(1). */

/* __torajs_str_at moved to torajs-str::transform::construct
 * (P3.1-e.4, 2026-05-23). ES2022 single-char Str; negative i
 * wraps; OOB returns empty Str. */

/* __torajs_str_replace / _replace_all moved to torajs-str::transform::replace
 * (P3.1-e.5, 2026-05-23). String-needle only (v0 subset, regex needle
 * not implemented). Empty-needle `replace` inserts at 0; empty-needle
 * `replaceAll` silently copies (spec throws TypeError — pre-existing
 * subset divergence preserved). Non-overlapping match consumption. */

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

/* __torajs_json_quote_str moved to torajs-str::json (P7.c, 2026-05-24).
 * __torajs_math_random / imul / clz32 / fround moved to torajs-num::math.
 * __torajs_print_i64_err / f64_err / bool_err moved to torajs-num::print_err.
 * __torajs_str_print_err lives in torajs-str::print (P3.1-g.1, 2026-05-23).
 * After P7.c, the runtime_str.c TU no longer owns JSON-quote or any console
 * .error primitive — they all live in Rust sub-crates.
 */

/* fs_* family + path_copy_to_buf helper moved to torajs-fs (P7.d,
 * 2026-05-24). Covers readFileSync / writeFileSync / appendFileSync /
 * existsSync / unlinkSync / mkdirSync / statSync.size / readdirSync.
 * process.* fns remain here until P7.e. */
#include <unistd.h>
#include <sys/stat.h>

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
    /* Inline NUL-copy: tora Str isn't NUL-terminated; libc getenv
     * needs a C string. path_copy_to_buf used to live here too but
     * moved to torajs-fs at P7.d. */
    char name[256];
    const uint8_t *p = __TORAJS_STR_CDATA(name_str);
    uint64_t nlen = __TORAJS_STR_LEN(name_str);
    if (nlen >= sizeof(name)) nlen = sizeof(name) - 1;
    memcpy(name, p, (size_t)nlen);
    name[nlen] = '\0';
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

/* __torajs_num_to_string_radix_{i,f} moved to torajs-num::tostring
 * (P3.2-c.3.a, 2026-05-23). f-version: NaN/Infinity sentinels +
 * integer-shortcut (route to _i) + 52-cap fractional digit loop.
 * i-version: standard divide-by-radix push-digits with i64::MIN-
 * safe two's-complement abs via i128 widen. */

/* __torajs_num_to_fixed_{i,f} / _to_exp_{i,f} / _to_precision_{i,f}
 * + the static js_normalize_exp_ helper moved to torajs-num::format
 * (P3.2-c.3.b, 2026-05-23). Pre-multiply + round-half-away-from-zero
 * for toFixed digits<16, Rust `{:e}` + exponent normalization for
 * toExp, manual %g (pre-format %e to pick form by actual exponent
 * + strip trailing zeros) for toPrecision. Special values
 * NaN/±Infinity preserve C-subset bit-for-bit ("nan"/"inf"/"-inf"),
 * spec-correctness ("NaN"/"Infinity") tracked in L3b backlog
 * alongside Math.round wedge. */

/* `Number.parseInt(s, radix)` — JS-spec parseInt, simplified subset.
 * Skips leading ASCII whitespace, accepts optional sign, then digits in
 * the given radix (2..36). Stops at the first non-digit. Returns NaN
 * encoded as the IEEE-754 quiet-NaN bit pattern when no digits are
 * consumed; otherwise the parsed double. radix=0 → autodetect (10
 * default; 16 if "0x"/"0X" prefix). */
/* __torajs_num_parse_int + _parse_float moved to torajs-num::parse
 * (P3.2-c.2, 2026-05-23). Rust impl removes the C version's 64-byte
 * input cap on parseFloat (scans for longest numeric prefix via
 * byte slice, no NUL-terminator dance). parseInt preserves the
 * trim+sign+0x-auto+radix scan bit-for-bit. */


/* Number predicates (is_nan/is_finite/is_integer/is_safe_integer × _i/_f =
 * 8 fns) moved to torajs-num::predicates (P3.2-c.1, 2026-05-23). */
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
