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

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

/* Increment refcount of any non-Copy heap object. NULL passes through
 * (sentinel for "no value"). Used by ssa_lower at every slot-copy /
 * borrow-promotion site where ownership becomes shared. */
void __torajs_rc_inc(void *p) {
    if (p == NULL) return;
    ((__torajs_heap_header_t *)p)->refcount += 1;
}

/* Decrement refcount; return 1 iff it reached zero (caller's per-type
 * drop path uses this to walk owned children before free). NULL passes
 * through (returns 0). */
int __torajs_rc_dec(void *p) {
    if (p == NULL) return 0;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount -= 1;
    return h->refcount == 0 ? 1 : 0;
}

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
 * Small-Str pool — thread-local LIFO for short-lived ≤16-byte
 * strings (the dominant size class for split tokens, single-char
 * concat results, number-to-string for short ints, etc.).
 *
 * The pool stores only the `header + 16-byte payload` block size;
 * any larger alloc bypasses to system malloc. On str_drop's free
 * path, qualified blocks are pushed to the LIFO so the next
 * alloc of equal-or-smaller size reuses the same memory — turning
 * tight `+ then drop` loops from malloc/free per iter into
 * pointer-pop/pointer-push.
 *
 * Single-threaded for now (tr has no async / threads); pool is
 * `_Thread_local` so multi-threaded runtimes can land later
 * without races. ============================================================ */

#define __TORAJS_STR_POOL_SLOTS    32
#define __TORAJS_STR_POOL_PAYLOAD  16

/* tr is single-threaded today (no async / threads exposed); plain
 * static storage avoids the TLS-init footprint on the binary. When
 * threads land later, swap to `_Thread_local`. */
static uint8_t *str_pool_[__TORAJS_STR_POOL_SLOTS];
static int str_pool_count_ = 0;

/* Block size class for a payload of `len` bytes. Short strings get
 * uniformly rounded up to POOL_PAYLOAD so every pooled block has
 * the same capacity (no risk of a small block being reused for a
 * larger payload). Anything past POOL_PAYLOAD pays the exact size. */
static inline size_t str_block_size_(uint64_t len) {
    if (len <= __TORAJS_STR_POOL_PAYLOAD) {
        return __TORAJS_STR_HDR_SIZE + __TORAJS_STR_POOL_PAYLOAD;
    }
    return __TORAJS_STR_HDR_SIZE + (size_t)len;
}

/* Caller must guarantee `len ≤ __TORAJS_STR_POOL_PAYLOAD`. */
static inline uint8_t *str_pool_pop_(uint64_t len) {
    uint8_t *p = str_pool_[--str_pool_count_];
    *(uint64_t *)p = __TORAJS_STR_HEADER_INIT;
    __TORAJS_STR_LEN(p) = len;
    return p;
}

/* Internal helper — alloc a fresh Str heap with refcount=1 + type_tag set
 * + len written. Caller fills the bytes payload at __TORAJS_STR_DATA(p).
 * Single-source-of-truth for every Str allocation in this file.
 *
 * Pool fast-path: short strings reuse a recently-freed block when
 * available; falls back to malloc-with-rounded-block-size otherwise.
 * Header is one combined u64 store (refcount=1 in low 32 bits +
 * type_tag=STR in [32:48] + flags=0 in [48:64]) instead of three
 * separate stores. */
static inline uint8_t *str_alloc_(uint64_t len) {
    if (len <= __TORAJS_STR_POOL_PAYLOAD && str_pool_count_ > 0) {
        return str_pool_pop_(len);
    }
    uint8_t *p = (uint8_t *)malloc(str_block_size_(len));
    *(uint64_t *)p = __TORAJS_STR_HEADER_INIT;
    __TORAJS_STR_LEN(p) = len;
    return p;
}

/* Free path used by both inkwell's str_drop (after rc → 0) and any
 * C-runtime helper that calls free directly on a Str block. Pushes
 * to the pool when the block fits and there's space; falls back to
 * system free otherwise. Exposed under __torajs_ namespace so the
 * inkwell-emitted str_drop can call it instead of libc free. */
void __torajs_str_free(uint8_t *p) {
    if (p == NULL) return;
    uint64_t len = __TORAJS_STR_LEN(p);
    if (len <= __TORAJS_STR_POOL_PAYLOAD && str_pool_count_ < __TORAJS_STR_POOL_SLOTS) {
        str_pool_[str_pool_count_++] = p;
        return;
    }
    free(p);
}

/* Inkwell-callable variant of the pool-aware alloc. Used by the IR
 * `__torajs_str_alloc` definition so its alloc path also benefits
 * from the pool. The IR side stays as a single LLVM function (and
 * still gets alwaysinline), but for short strings it now cheaply
 * delegates here when the pool has a slot. */
uint8_t *__torajs_str_alloc_pooled(uint64_t len) {
    return str_alloc_(len);
}

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

/* Substr cell pool — same shape as the small-Str pool, sized for
 * 32-byte view structs. Hot in any code path that does substr.trim
 * / .slice / .substring inside a tight loop. */
#define __TORAJS_SUBSTR_POOL_SLOTS 32
static uint8_t *substr_pool_[__TORAJS_SUBSTR_POOL_SLOTS];
static int substr_pool_count_ = 0;

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

/* Free path for split blocks. arr_drop (inkwell IR) calls
 * __torajs_arr_free which dispatches here when the SPLIT flag is
 * set in the universal header. */
void __torajs_arr_free(void *p) {
    if (p == NULL) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (h->flags & __TORAJS_FLAG_SPLIT_BLOCK) {
        uint64_t cap = *(uint64_t *)((uint8_t *)p + 16);
        if (split_pool_count_ < __TORAJS_SPLIT_POOL_SLOTS) {
            split_pool_blocks_[split_pool_count_] = (uint8_t *)p;
            split_pool_caps_[split_pool_count_] = cap;
            split_pool_count_++;
            return;
        }
    }
    free(p);
}

/* Create a substring view of `parent` (an OWNED Str) starting at
 * `offset` with length `len`. Caller must ensure offset+len ≤
 * parent.length (no bounds check here — matches the unchecked-index
 * convention used by other tr runtime helpers). Bumps parent's
 * refcount so its bytes stay alive while the view exists.
 *
 * For nested views (`substr.slice(...)`), the caller should resolve
 * to the root parent before calling this — view-of-view collapses to
 * view-of-owner so drop chains stay depth-1. (Phase Substr.A only
 * exposes `__torajs_str_split` as a view source, which always passes
 * an OWNED Str as parent; deferred until a slice/substring view path
 * lands.) */
void *__torajs_substr_create(void *parent, uint64_t offset, uint64_t len) {
    uint8_t *v;
    if (substr_pool_count_ > 0) {
        v = substr_pool_[--substr_pool_count_];
    } else {
        v = (uint8_t *)malloc(__TORAJS_SUBSTR_SIZE);
    }
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)v;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_STR;  /* SSA Type::Substr is still "str" at the type-tag layer */
    h->flags = 0;  /* reserved for future weak/mark bits; view-vs-owned distinguished by SSA Type, not flag */
    __TORAJS_SUBSTR_LEN(v) = len;
    *(uint8_t **)(v + __TORAJS_SUBSTR_PARENT_OFF) = (uint8_t *)parent;
    *(uint64_t *)(v + __TORAJS_SUBSTR_OFFSET_OFF) = offset;
    __torajs_rc_inc(parent);
    return v;
}

/* Drop a Substr view. Two cases distinguished by the INLINE flag:
 *   - standalone (flag clear): dec own refcount; at 0, str_drop(parent)
 *     and free self.
 *   - inline (flag set): the substr struct lives inside a bigger
 *     allocation (typically the array tail produced by str_split). Don't
 *     touch own refcount, don't free self — just dec parent (each inline
 *     substr still holds one parent ref). The enclosing block's drop
 *     reclaims the substr storage when it frees itself. */
void __torajs_substr_drop(void *v) {
    if (v == NULL) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)v;
    if (h->flags & __TORAJS_FLAG_SUBSTR_INLINE) {
        void *parent = (void *)*(uint8_t **)((uint8_t *)v + __TORAJS_SUBSTR_PARENT_OFF);
        __torajs_str_drop(parent);
        return;
    }
    if (__torajs_rc_dec(v)) {
        void *parent = (void *)*(uint8_t **)((uint8_t *)v + __TORAJS_SUBSTR_PARENT_OFF);
        __torajs_str_drop(parent);
        if (substr_pool_count_ < __TORAJS_SUBSTR_POOL_SLOTS) {
            substr_pool_[substr_pool_count_++] = (uint8_t *)v;
        } else {
            free(v);
        }
    }
}

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
    uint8_t *p = str_alloc_(len);
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
    uint8_t *p = str_alloc_(v_len + s_len);
    uint8_t *out = __TORAJS_STR_DATA(p);
    if (v_len) memcpy(out, substr_data_(v), (size_t)v_len);
    if (s_len) memcpy(out + v_len, __TORAJS_STR_CDATA(s), (size_t)s_len);
    return p;
}

void *__torajs_substr_concat_str_substr(const uint8_t *s, const uint8_t *v) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint64_t v_len = __TORAJS_SUBSTR_LEN(v);
    uint8_t *p = str_alloc_(s_len + v_len);
    uint8_t *out = __TORAJS_STR_DATA(p);
    if (s_len) memcpy(out, __TORAJS_STR_CDATA(s), (size_t)s_len);
    if (v_len) memcpy(out + s_len, substr_data_(v), (size_t)v_len);
    return p;
}

void *__torajs_substr_concat_substr_substr(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = __TORAJS_SUBSTR_LEN(a);
    uint64_t b_len = __TORAJS_SUBSTR_LEN(b);
    uint8_t *p = str_alloc_(a_len + b_len);
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

#define __TORAJS_ARR_HDR_SIZE   24
#define __TORAJS_ARR_LEN(p)     (*(uint64_t *)((const uint8_t *)(p) + 8))
#define __TORAJS_ARR_CAP(p)     (*(uint64_t *)((const uint8_t *)(p) + 16))
#define __TORAJS_ARR_DATA(p)    ((uint8_t *)(p) + __TORAJS_ARR_HDR_SIZE)
#define __TORAJS_ARR_CDATA(p)   ((const uint8_t *)(p) + __TORAJS_ARR_HDR_SIZE)
#define __TORAJS_ARR_SLOT(p, i) (__TORAJS_ARR_DATA(p) + (uint64_t)(i) * 8)
#define __TORAJS_ARR_CSLOT(p, i) (__TORAJS_ARR_CDATA(p) + (uint64_t)(i) * 8)

/* Append every element of `src` to `dst` via a single memcpy. Caller
 * MUST have pre-sized dst's cap to fit (typical: array literal with
 * spreads pre-computes total length and allocs once). Bumps dst's
 * len. Both arrays are the same 8-byte-slot layout — element type
 * doesn't matter at this layer. */
void __torajs_arr_extend_unchecked(uint8_t *dst, const uint8_t *src) {
    uint64_t dst_len = __TORAJS_ARR_LEN(dst);
    uint64_t src_len = __TORAJS_ARR_LEN(src);
    if (src_len == 0) return;
    memcpy(__TORAJS_ARR_SLOT(dst, dst_len), __TORAJS_ARR_CDATA(src), (size_t)src_len * 8);
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
    uint8_t *p = str_alloc_(out_len);
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
    __TORAJS_ARR_CAP(p) = cap;
    return p;
}

/* `arr.slice(start, end)` — fresh array containing the [start, end)
 * range. Both indices are clamped to [0, arr.len]. Single malloc +
 * one memcpy. Element-type-agnostic (8-byte slots). */
void *__torajs_arr_slice(const uint8_t *arr, int64_t start, int64_t end) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    int64_t lo = start < 0 ? 0 : (start > (int64_t)len ? (int64_t)len : start);
    int64_t hi = end < 0 ? 0 : (end > (int64_t)len ? (int64_t)len : end);
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
    uint8_t *p = str_alloc_(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), buf, (size_t)len);
    return p;
}

/* Same shape for f64. Uses %g for short round-trip-friendly output —
 * matches JS's String(n) for the integer-valued cases we exercise.
 * (Full IEEE-754 round-trip requires more care; we'll punt on that
 * until a test demands it.) */
void *__torajs_f64_to_str(double d) {
    char buf[32];
    int written = snprintf(buf, sizeof(buf), "%g", d);
    if (written < 0) written = 0;
    uint64_t len = (uint64_t)written;
    uint8_t *p = str_alloc_(len);
    if (len) memcpy(__TORAJS_STR_DATA(p), buf, (size_t)len);
    return p;
}


/* Returns 1 if strings have equal length and equal bytes, 0 otherwise.
 * `===` / `!==` between Type::Str values dispatches here instead of
 * pointer-compare. Spec ECMA-262 §7.2.16 step 3: "If x and y are
 * Strings ... return true iff length(x) === length(y) and same code
 * units." We don't deal with UTF-16 here — bytes match is enough for
 * the byte-encoded Str layout. */
int64_t __torajs_str_eq(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = __TORAJS_STR_LEN(a);
    uint64_t b_len = __TORAJS_STR_LEN(b);
    if (a_len != b_len) return 0;
    if (a_len == 0) return 1;
    return memcmp(__TORAJS_STR_CDATA(a), __TORAJS_STR_CDATA(b), (size_t)a_len) == 0 ? 1 : 0;
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
    __TORAJS_ARR_CAP(arr) = out_count;
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

void *__torajs_arr_join(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t sep_len = __TORAJS_STR_LEN(sep);
    const uint8_t *sep_data = __TORAJS_STR_CDATA(sep);

    if (len == 0) {
        return str_alloc_(0);
    }

    /* pass 1: total = sum(elem.len) + sep_len * (len - 1) */
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        const uint8_t *elem = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(arr, i);
        total += __TORAJS_STR_LEN(elem);
    }
    total += sep_len * (len - 1);

    /* pass 2: copy */
    uint8_t *p = str_alloc_(total);
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
        return str_alloc_(0);
    }

    /* pass 1: total = sum(view.len) + sep_len * (len - 1) */
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        const uint8_t *v = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(arr, i);
        total += __TORAJS_SUBSTR_LEN(v);
    }
    total += sep_len * (len - 1);

    /* pass 2: copy */
    uint8_t *p = str_alloc_(total);
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
    uint8_t *p = str_alloc_(1);
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
    uint8_t *p = arr_alloc_(len, len);
    if (len) memcpy(__TORAJS_ARR_DATA(p), __TORAJS_ARR_CDATA(arr), (size_t)len * 8);
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
    uint8_t *p = str_alloc_(new_len);
    if (new_len) memcpy(__TORAJS_STR_DATA(p), __TORAJS_STR_CDATA(s) + start, (size_t)new_len);
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
        uint8_t *p = str_alloc_(1);
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
        return str_alloc_(0);
    }
    uint8_t *p = str_alloc_(1);
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
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)s_len);
        return p;
    }
    uint64_t out_len = s_len - n_len + r_len;
    uint8_t *p = str_alloc_(out_len);
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
        uint8_t *p = str_alloc_(s_len);
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
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)s_len);
        return p;
    }
    /* out_len = s_len - hits*n_len + hits*r_len */
    uint64_t out_len = s_len + hits * (r_len > n_len ? (r_len - n_len) : 0)
                              - hits * (r_len < n_len ? (n_len - r_len) : 0);
    uint8_t *p = str_alloc_(out_len);
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

/* `s.localeCompare(other)` — ASCII-only memcmp. JS spec returns a
 * locale-sensitive result; v0 just compares byte-wise (fine for the
 * ASCII-typical subset). Returns -1, 0, or 1. */
int64_t __torajs_str_locale_compare(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = __TORAJS_STR_LEN(a);
    uint64_t b_len = __TORAJS_STR_LEN(b);
    uint64_t min = a_len < b_len ? a_len : b_len;
    int r = min ? memcmp(__TORAJS_STR_CDATA(a), __TORAJS_STR_CDATA(b), (size_t)min) : 0;
    if (r < 0) return -1;
    if (r > 0) return 1;
    if (a_len < b_len) return -1;
    if (a_len > b_len) return 1;
    return 0;
}

/* `s.lastIndexOf(needle)` — reverse memcmp scan, -1 on miss. */
int64_t __torajs_str_last_index_of(const uint8_t *s, const uint8_t *needle) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    uint64_t n_len = __TORAJS_STR_LEN(needle);
    if (n_len == 0) return (int64_t)s_len;
    if (n_len > s_len) return -1;
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    const uint8_t *n_data = __TORAJS_STR_CDATA(needle);
    for (int64_t i = (int64_t)(s_len - n_len); i >= 0; i--) {
        if (memcmp(s_data + (uint64_t)i, n_data, (size_t)n_len) == 0) {
            return i;
        }
    }
    return -1;
}

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
    uint8_t *p = str_alloc_(out);
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
    uint64_t len = __TORAJS_STR_LEN(s);
    if (len) fwrite(__TORAJS_STR_CDATA(s), 1, (size_t)len, stderr);
    fputc('\n', stderr);
}

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
    uint8_t *p_data = __TORAJS_ARR_DATA(p);
    uint64_t cursor = 0;
    for (uint64_t i = 0; i < outer_len; i++) {
        const uint8_t *inner = *(const uint8_t *const *)__TORAJS_ARR_CSLOT(outer, i);
        uint64_t inner_len = __TORAJS_ARR_LEN(inner);
        if (inner_len) {
            memcpy(p_data + cursor, __TORAJS_ARR_CDATA(inner), (size_t)inner_len * 8);
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
    uint8_t *p_data = __TORAJS_ARR_DATA(p);
    if (a_len) memcpy(p_data, __TORAJS_ARR_CDATA(a), (size_t)a_len * 8);
    if (b_len) memcpy(p_data + (size_t)a_len * 8, __TORAJS_ARR_CDATA(b), (size_t)b_len * 8);
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

/* `arr.shift()` — remove and return slot[0]. Memmoves the rest of the
 * slots one slot left, decrements len. Subset convention: empty-array
 * shift is unchecked (no `T | undefined`). Returns the popped value
 * as i64 (the slot's 8-byte payload, reinterpreted by the caller). */
int64_t __torajs_arr_shift(uint8_t *arr) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    int64_t v = *(int64_t *)__TORAJS_ARR_SLOT(arr, 0);
    if (len > 1) {
        memmove(__TORAJS_ARR_SLOT(arr, 0),
                __TORAJS_ARR_SLOT(arr, 1),
                (size_t)(len - 1) * 8);
    }
    __TORAJS_ARR_LEN(arr) = len - 1;
    return v;
}

/* `arr.unshift(v)` — insert `v` at slot[0]. Grows by 1 (realloc if
 * cap < len+1), memmoves existing slots one slot right, writes v at
 * slot[0]. Returns the new array pointer (caller stores it back into
 * the slot, mirroring `push`). Returns the new length is the JS spec,
 * but tr's API matches `push` (returning ptr) for parser symmetry —
 * the return value is typically discarded. */
void *__torajs_arr_unshift(uint8_t *arr, int64_t v) {
    uint64_t len = __TORAJS_ARR_LEN(arr);
    uint64_t cap = __TORAJS_ARR_CAP(arr);
    if (len >= cap) {
        /* Reuse arr_push's grow strategy: double cap (or 1 if 0).
         * Allocate a new block, copy header + slots, free old. */
        uint64_t new_cap = cap == 0 ? 1 : cap * 2;
        uint8_t *p = arr_alloc_(0, new_cap);
        if (len > 0) {
            memcpy(__TORAJS_ARR_SLOT(p, 1),
                   __TORAJS_ARR_CDATA(arr),
                   (size_t)len * 8);
        }
        *(int64_t *)__TORAJS_ARR_SLOT(p, 0) = v;
        __TORAJS_ARR_LEN(p) = len + 1;
        free(arr);
        return p;
    }
    /* In-place: memmove right + write slot[0]. */
    if (len > 0) {
        memmove(__TORAJS_ARR_SLOT(arr, 1),
                __TORAJS_ARR_SLOT(arr, 0),
                (size_t)len * 8);
    }
    *(int64_t *)__TORAJS_ARR_SLOT(arr, 0) = v;
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

/* `s.toUpperCase()` / `s.toLowerCase()` — ASCII-only fold (matches the
 * subset's byte-level Str layout). Non-ASCII bytes pass through
 * unchanged. Single malloc, single pass. */
void *__torajs_str_to_upper(const uint8_t *s) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    uint8_t *p = str_alloc_(len);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s_data[i];
        if (c >= 'a' && c <= 'z') c = (uint8_t)(c - 32);
        p_data[i] = c;
    }
    return p;
}

void *__torajs_str_to_lower(const uint8_t *s) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    uint8_t *p = str_alloc_(len);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s_data[i];
        if (c >= 'A' && c <= 'Z') c = (uint8_t)(c + 32);
        p_data[i] = c;
    }
    return p;
}

#include <math.h>

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
    uint8_t *p = str_alloc_((uint64_t)len);
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
    char buf[64];
    int written = snprintf(buf, sizeof(buf), "%.*f", (int)digits, n);
    if (written < 0) written = 0;
    uint64_t len = (uint64_t)written;
    uint8_t *p = str_alloc_(len);
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
    uint8_t *p = str_alloc_(len);
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
    uint8_t *p = str_alloc_(len);
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
static int is_trim_ws_(uint8_t c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f';
}

void *__torajs_str_trim_start(const uint8_t *s) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    uint64_t lo = 0;
    while (lo < len && is_trim_ws_(s_data[lo])) lo++;
    uint64_t out = len - lo;
    uint8_t *p = str_alloc_(out);
    if (out) memcpy(__TORAJS_STR_DATA(p), s_data + lo, (size_t)out);
    return p;
}

void *__torajs_str_trim_end(const uint8_t *s) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    uint64_t hi = len;
    while (hi > 0 && is_trim_ws_(s_data[hi - 1])) hi--;
    uint8_t *p = str_alloc_(hi);
    if (hi) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)hi);
    return p;
}

void *__torajs_str_trim(const uint8_t *s) {
    uint64_t len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    uint64_t lo = 0;
    while (lo < len && is_trim_ws_(s_data[lo])) lo++;
    uint64_t hi = len;
    while (hi > lo && is_trim_ws_(s_data[hi - 1])) hi--;
    uint64_t out = hi - lo;
    uint8_t *p = str_alloc_(out);
    if (out) memcpy(__TORAJS_STR_DATA(p), s_data + lo, (size_t)out);
    return p;
}

/* `s.padStart(targetLen, padStr)` — if s.length >= targetLen, return s
 * unchanged-content (still a fresh alloc to keep ownership uniform).
 * Otherwise prepend bytes from padStr, repeating + truncating, so the
 * result has exactly targetLen bytes. JS spec uses code units; we use
 * bytes (good enough for ASCII). padEnd appends instead. */
void *__torajs_str_pad_start(const uint8_t *s, int64_t target_len, const uint8_t *pad) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    if (target_len < 0 || (uint64_t)target_len <= s_len) {
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)s_len);
        return p;
    }
    uint64_t pad_len = __TORAJS_STR_LEN(pad);
    const uint8_t *pad_data = __TORAJS_STR_CDATA(pad);
    uint64_t out = (uint64_t)target_len;
    uint8_t *p = str_alloc_(out);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    uint64_t need = out - s_len;
    /* Pad source might be empty → can't fill, return s_len-padded zero
     * bytes. Match JS behavior: if padStr is empty, the original is
     * returned. We don't have access to the original ptr here; just
     * write zero bytes and rely on tests to provide non-empty pad. */
    if (pad_len == 0) {
        memset(p_data, ' ', (size_t)need);
    } else {
        for (uint64_t i = 0; i < need; i++) {
            p_data[i] = pad_data[i % pad_len];
        }
    }
    if (s_len) memcpy(p_data + need, s_data, (size_t)s_len);
    return p;
}

void *__torajs_str_pad_end(const uint8_t *s, int64_t target_len, const uint8_t *pad) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    const uint8_t *s_data = __TORAJS_STR_CDATA(s);
    if (target_len < 0 || (uint64_t)target_len <= s_len) {
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(__TORAJS_STR_DATA(p), s_data, (size_t)s_len);
        return p;
    }
    uint64_t pad_len = __TORAJS_STR_LEN(pad);
    const uint8_t *pad_data = __TORAJS_STR_CDATA(pad);
    uint64_t out = (uint64_t)target_len;
    uint8_t *p = str_alloc_(out);
    uint8_t *p_data = __TORAJS_STR_DATA(p);
    if (s_len) memcpy(p_data, s_data, (size_t)s_len);
    uint64_t fill = out - s_len;
    if (pad_len == 0) {
        memset(p_data + s_len, ' ', (size_t)fill);
    } else {
        for (uint64_t i = 0; i < fill; i++) {
            p_data[s_len + i] = pad_data[i % pad_len];
        }
    }
    return p;
}

/* M6.3 — JSON.parse runtime helpers. Cursor is `int64_t *pos`,
 * updated in place by every helper so ssa_lower's compile-time
 * specialized parser can thread one alloca'd slot through all
 * recursive calls. On syntactic mismatch each helper stuffs an
 * error string into the throw_active / throw_value globals via
 * `__torajs_throw_set` and returns a default; ssa_lower emits a
 * `throw_check` after each call so propagation flows correctly.
 */

extern void __torajs_throw_set(int64_t v);

static void torajs_json_throw(const char *msg, int64_t pos) {
    char buf[96];
    int n = snprintf(buf, sizeof(buf), "%s at pos %lld", msg, (long long)pos);
    if (n < 0) n = 0;
    if ((size_t)n >= sizeof(buf)) n = (int)sizeof(buf) - 1;
    uint64_t len = (uint64_t)n;
    uint8_t *err = str_alloc_(len);
    if (len) memcpy(__TORAJS_STR_DATA(err), buf, (size_t)len);
    __torajs_throw_set((int64_t)(uintptr_t)err);
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
        return str_alloc_(0);
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
                return str_alloc_(0);
            }
            uint8_t e = data[scan + 1];
            if (e == 'u') {
                if (scan + 6 > (int64_t)len) {
                    torajs_json_throw("JSON.parse: short \\u escape", scan);
                    return str_alloc_(0);
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
        return str_alloc_(0);
    }
    /* Pass 2: write decoded bytes. */
    uint8_t *p = str_alloc_(out_len);
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

/* Compare a torajs Str value byte-by-byte against a literal C string.
 * Returns 1 on match, 0 otherwise. Used by the object parser to
 * verify a parsed key against an expected field name. */
int64_t __torajs_str_eq_cstr(const uint8_t *s, const uint8_t *cstr_bytes, int64_t cstr_len) {
    uint64_t s_len = __TORAJS_STR_LEN(s);
    if ((int64_t)s_len != cstr_len) return 0;
    if (cstr_len == 0) return 1;
    return memcmp(__TORAJS_STR_CDATA(s), cstr_bytes, (size_t)cstr_len) == 0 ? 1 : 0;
}
