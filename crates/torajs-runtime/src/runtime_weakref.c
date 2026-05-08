/*
 * runtime_weakref.c — torajs T-26 (v0.7) WeakRef substrate.
 *
 * `new WeakRef(target)` creates a heap struct that observes
 * `target` without keeping it alive. When `target`'s strong rc
 * transitions to zero (via __torajs_rc_dec), the runtime walks the
 * registry below and clears every WeakRef pointing at it to NULL.
 * Subsequent `wr.deref()` calls return null instead of a dangling
 * pointer.
 *
 * Heap layout (16 bytes):
 *
 *     [universal_heap_header (8B)] [target ptr (8B)]
 *
 *   - target = NULL  → target was reclaimed; deref returns null
 *   - target != NULL → target still strong-reachable; deref returns
 *                       the pointer with strong rc bumped (the
 *                       caller assumes ownership)
 *
 * Tradeoff vs. Swift-/Rust-style "weak count in object header":
 * the strong-rc-only header keeps the alloc layout stable across
 * the entire ARC system (no 8B header expansion → no relayout of
 * Promise/Date/Symbol/etc). The price is a hashmap probe inside
 * rc_dec when a heap object dies. We gate that probe on a
 * non-zero `weak_ref_active` counter so non-WeakRef-using programs
 * pay one untaken branch per dec — same as Python's tp_weaklistoffset
 * approach when a type opts out.
 *
 * Cycle collector (Bacon & Rajan trial deletion) lands as a
 * separate slice of T-26 — that one needs the color-bit reservation
 * in the universal header's flags field (3 free bits available
 * today). WeakRef ships first because it's substrate-independent;
 * cycles + WeakMap/WeakSet build on the same registry below.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_WEAKREF 11

extern void __torajs_rc_inc(void *p);
extern int  __torajs_rc_dec(void *p);

typedef struct {
    __torajs_heap_header_t header;
    void *target;
} WeakRef;

/* ============================================================
 * Registry — open-addressing on (target → linked list of WeakRefs).
 *
 * Each bucket is a linked list of `Bucket` cells; each cell stores
 * one (target, list-of-weakrefs-pointing-at-it) record. We keep
 * one cell per target rather than per-WeakRef so the dying-target
 * walk is a single bucket lookup + freeing the cell, not an O(n)
 * full-table scan.
 *
 * 1024 buckets is intentionally small — the typical program has
 * ≤ a few hundred live WeakRefs. The hash dispersion (double-mix)
 * keeps chains short. Resize is a follow-up; only matters if a
 * program holds millions of weakrefs simultaneously.
 * ============================================================ */

#define WEAKREF_BUCKETS 1024

typedef struct WeakRefNode {
    WeakRef *wr;
    struct WeakRefNode *next;
} WeakRefNode;

typedef struct TargetCell {
    void *target;
    WeakRefNode *refs;
    struct TargetCell *next;  /* hash chain */
} TargetCell;

static TargetCell *g_buckets[WEAKREF_BUCKETS];
static uint64_t g_active = 0;  /* total live WeakRefs */

static inline uint32_t hash_ptr(void *p) {
    uintptr_t v = (uintptr_t)p;
    v ^= v >> 33; v *= 0xff51afd7ed558ccdULL;
    v ^= v >> 33; v *= 0xc4ceb9fe1a85ec53ULL;
    v ^= v >> 33;
    return (uint32_t)(v & (WEAKREF_BUCKETS - 1));
}

static TargetCell *registry_find(void *target, uint32_t bkt) {
    TargetCell *cur = g_buckets[bkt];
    while (cur) {
        if (cur->target == target) return cur;
        cur = cur->next;
    }
    return NULL;
}

static TargetCell *registry_get_or_alloc(void *target, uint32_t bkt) {
    TargetCell *c = registry_find(target, bkt);
    if (c) return c;
    c = (TargetCell *)malloc(sizeof(TargetCell));
    c->target = target;
    c->refs = NULL;
    c->next = g_buckets[bkt];
    g_buckets[bkt] = c;
    return c;
}

static void registry_remove_cell(TargetCell *c, uint32_t bkt) {
    TargetCell **slot = &g_buckets[bkt];
    while (*slot) {
        if (*slot == c) { *slot = c->next; break; }
        slot = &(*slot)->next;
    }
    free(c);
}

static void registry_register(WeakRef *wr) {
    uint32_t bkt = hash_ptr(wr->target);
    TargetCell *c = registry_get_or_alloc(wr->target, bkt);
    WeakRefNode *n = (WeakRefNode *)malloc(sizeof(WeakRefNode));
    n->wr = wr;
    n->next = c->refs;
    c->refs = n;
    g_active += 1;
}

static void registry_deregister(WeakRef *wr) {
    if (wr->target == NULL) return;  /* already cleared by target_dying */
    uint32_t bkt = hash_ptr(wr->target);
    TargetCell *c = registry_find(wr->target, bkt);
    if (!c) return;
    WeakRefNode **slot = &c->refs;
    while (*slot) {
        if ((*slot)->wr == wr) {
            WeakRefNode *gone = *slot;
            *slot = gone->next;
            free(gone);
            g_active -= 1;
            break;
        }
        slot = &(*slot)->next;
    }
    if (c->refs == NULL) registry_remove_cell(c, bkt);
}

/* ============================================================
 * Public API.
 * ============================================================ */

/* Called by rc_dec when a heap object's strong rc transitions to
 * zero. Walks every WeakRef registered against this target,
 * sets its `target` to NULL, and frees the bucket cell. */
void __torajs_weakref_target_dying(void *target) {
    if (g_active == 0) return;
    uint32_t bkt = hash_ptr(target);
    TargetCell *c = registry_find(target, bkt);
    if (!c) return;
    WeakRefNode *cur = c->refs;
    while (cur) {
        cur->wr->target = NULL;
        WeakRefNode *next = cur->next;
        free(cur);
        g_active -= 1;
        cur = next;
    }
    registry_remove_cell(c, bkt);
}

/* `new WeakRef(target)` — allocate a fresh +1-rc WeakRef and
 * register it. The target is NOT rc_inc'd; the WeakRef is observed
 * via the registry instead. Caller's ownership of `target` is
 * unchanged — they still own it as before. */
void *__torajs_weakref_create(void *target) {
    WeakRef *wr = (WeakRef *)malloc(sizeof(WeakRef));
    wr->header.refcount = 1;
    wr->header.type_tag = __TORAJS_TAG_WEAKREF;
    wr->header.flags = 0;
    wr->target = target;
    /* NULL target is legal in the spec only for some prototypes; we
     * accept it here without registering (deref always returns null). */
    if (target != NULL) {
        registry_register(wr);
    }
    return wr;
}

/* `wr.deref()` — return the target if still alive, NULL otherwise.
 * Bumps the strong rc on success so the caller assumes ownership. */
void *__torajs_weakref_deref(void *p) {
    if (!p) return NULL;
    WeakRef *wr = (WeakRef *)p;
    void *t = wr->target;
    if (t != NULL) {
        __torajs_rc_inc(t);
    }
    return t;
}

/* rc-aware drop. Called from value_drop_heap's TAG_WEAKREF case. */
void __torajs_weakref_drop(void *p) {
    if (!p) return;
    WeakRef *wr = (WeakRef *)p;
    if (wr->header.flags & 4 /* STATIC_LITERAL */) return;
    wr->header.refcount -= 1;
    if (wr->header.refcount == 0) {
        registry_deregister(wr);
        free(wr);
    }
}
