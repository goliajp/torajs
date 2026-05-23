/*
 * runtime_weakref.c — shared observer registry for the weak-ref
 * family (WeakRef + WeakMap + WeakSet).
 *
 * The owner-side WeakRef ops (`create` / `deref` / `drop`) live in
 * `crates/torajs-weak/` (P4.3'-a, 2026-05-24). What remains here:
 *
 *   - the WeakRef struct typedef (used by `target_dying` below to
 *     clear `wr->target` on the OBSERVER_WEAKREF dispatch path).
 *     ABI-shared with `torajs_weak::layout::WeakRef`.
 *   - the process-global target → observer registry
 *     (`g_buckets[WEAKREF_BUCKETS]`, hash chain + per-target linked
 *     list of `(kind, owner)` observer nodes).
 *   - `registry_register` / `registry_deregister` — Rust-side
 *     `torajs-weak` calls into these via `extern "C"`.
 *   - `target_dying` — called by `__torajs_rc_dec` / the inlined Obj
 *     drop walk in `ssa_lower` when any heap target's strong rc
 *     transitions to zero; broadcasts cleanup across every observer.
 *
 * P4.3'-b ports the registry itself to Rust; this file disappears at
 * P4.3'-e along with `runtime_weakmap.c` and `runtime_weakset.c`.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

/* Cleanup hooks defined in runtime_weakmap.c / runtime_weakset.c.
 * Either is allowed to be missing in a given binary (linker would
 * fail otherwise) — but since both modules are always compiled
 * into the runtime, the symbols always resolve. The hooks are
 * called only when at least one map/set has registered against
 * the dying target, so the observer-kind dispatch fires the right
 * one. */
extern void __torajs_weakmap_invalidate_key(void *owner, void *dying_key);
extern void __torajs_weakset_invalidate_key(void *owner, void *dying_key);

typedef struct {
    __torajs_heap_header_t header;
    void *target;
} WeakRef;

/* ============================================================
 * Shared observer registry. (target → linked list of observers).
 * Each observer carries `kind` + `owner`; weakref_target_dying
 * walks the cell's list and dispatches per kind.
 * ============================================================ */

#define WEAKREF_BUCKETS 1024

typedef enum {
    OBSERVER_WEAKREF = 0,
    OBSERVER_WEAKMAP = 1,
    OBSERVER_WEAKSET = 2,
} ObserverKind;

typedef struct ObserverNode {
    ObserverKind kind;
    void *owner;       /* WeakRef* / WeakMap* / WeakSet* */
    struct ObserverNode *next;
} ObserverNode;

typedef struct TargetCell {
    void *target;
    ObserverNode *observers;
    struct TargetCell *next;  /* hash chain */
} TargetCell;

static TargetCell *g_buckets[WEAKREF_BUCKETS];
static uint64_t g_active = 0;  /* total live observers */

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
    c->observers = NULL;
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

/* Internal — public-to-other-runtime-modules. WeakMap.set /
 * WeakSet.add register here; on entry removal they deregister. */
void __torajs_weakref_registry_register(void *target, ObserverKind kind, void *owner) {
    if (target == NULL) return;
    uint32_t bkt = hash_ptr(target);
    TargetCell *c = registry_get_or_alloc(target, bkt);
    ObserverNode *n = (ObserverNode *)malloc(sizeof(ObserverNode));
    n->kind = kind;
    n->owner = owner;
    n->next = c->observers;
    c->observers = n;
    g_active += 1;
}

/* Internal — remove a specific (target, kind, owner) tuple. Used
 * by WeakMap.delete / WeakSet.delete to keep the registry tidy
 * when the entry is explicitly removed (vs. cleared by the
 * dying-target walk). Tolerant: returns silently if no match. */
void __torajs_weakref_registry_deregister(void *target, ObserverKind kind, void *owner) {
    if (target == NULL) return;
    uint32_t bkt = hash_ptr(target);
    TargetCell *c = registry_find(target, bkt);
    if (!c) return;
    ObserverNode **slot = &c->observers;
    while (*slot) {
        if ((*slot)->kind == kind && (*slot)->owner == owner) {
            ObserverNode *gone = *slot;
            *slot = gone->next;
            free(gone);
            g_active -= 1;
            break;
        }
        slot = &(*slot)->next;
    }
    if (c->observers == NULL) registry_remove_cell(c, bkt);
}

/* ============================================================
 * Public API — called by rc_dec / Obj-drop walk_blk and by
 * ssa_lower-emitted IR.
 * ============================================================ */

/* Called when a heap object's strong rc transitions to zero.
 * Walks every observer registered against this target and
 * dispatches per kind. Cells / nodes free as they're processed. */
void __torajs_weakref_target_dying(void *target) {
    if (g_active == 0) return;
    uint32_t bkt = hash_ptr(target);
    TargetCell *c = registry_find(target, bkt);
    if (!c) return;
    ObserverNode *cur = c->observers;
    while (cur) {
        switch (cur->kind) {
            case OBSERVER_WEAKREF: {
                ((WeakRef *)cur->owner)->target = NULL;
                break;
            }
            case OBSERVER_WEAKMAP: {
                __torajs_weakmap_invalidate_key(cur->owner, target);
                break;
            }
            case OBSERVER_WEAKSET: {
                __torajs_weakset_invalidate_key(cur->owner, target);
                break;
            }
        }
        ObserverNode *next = cur->next;
        free(cur);
        g_active -= 1;
        cur = next;
    }
    registry_remove_cell(c, bkt);
}
