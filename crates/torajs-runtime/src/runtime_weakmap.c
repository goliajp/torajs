/*
 * runtime_weakmap.c — torajs T-26.B (v0.7) WeakMap.
 *
 * `new WeakMap()` returns a heap struct holding an internal hash
 * table keyed by the **pointer identity** of heap objects. Set,
 * get, has, delete are all O(1) average. Each entry registers
 * itself in the shared weakref registry under the key; when the
 * key dies, the registry's broadcast invokes
 * __torajs_weakmap_invalidate_key which removes the entry.
 *
 * Heap layout:
 *
 *     [universal_heap_header (8B)]
 *     [bucket count u32]
 *     [entry count u32]
 *     [bucket array ptr (8B)]   — buckets[N], each = WeakMapEntry*
 *
 * Buckets start at 16 and grow on load > 0.75. Resize is in-place:
 * allocate a new bucket array, rehash, free the old. Entries
 * (`WeakMapEntry`) are owned by the map and freed on delete /
 * invalidate / drop; the value held inside an entry is rc-inc'd
 * on insert and rc-dec'd on entry destruction (via
 * value_drop_heap, so the value's per-type free runs).
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_WEAKMAP 12

extern void __torajs_rc_inc(void *p);
extern void __torajs_value_drop_heap(void *p);

/* From runtime_weakref.c — shared registry. */
typedef enum {
    OBSERVER_WEAKREF = 0,
    OBSERVER_WEAKMAP = 1,
    OBSERVER_WEAKSET = 2,
} ObserverKind;
extern void __torajs_weakref_registry_register(void *target, ObserverKind kind, void *owner);
extern void __torajs_weakref_registry_deregister(void *target, ObserverKind kind, void *owner);

typedef struct WeakMapEntry {
    void *key;          /* observed via registry; not rc'd */
    void *value;        /* strong-rc'd while in the map */
    struct WeakMapEntry *next;
} WeakMapEntry;

typedef struct {
    __torajs_heap_header_t header;
    uint32_t n_buckets;
    uint32_t n_entries;
    WeakMapEntry **buckets;
} WeakMap;

/* ============================================================
 * Hashing + bucket management.
 * ============================================================ */

static inline uint32_t hash_ptr_for(void *p, uint32_t n_buckets) {
    uintptr_t v = (uintptr_t)p;
    v ^= v >> 33; v *= 0xff51afd7ed558ccdULL;
    v ^= v >> 33; v *= 0xc4ceb9fe1a85ec53ULL;
    v ^= v >> 33;
    return (uint32_t)(v & (n_buckets - 1));
}

#define WEAKMAP_INITIAL_BUCKETS 16

static WeakMapEntry *weakmap_find(WeakMap *m, void *key, uint32_t bkt) {
    WeakMapEntry *cur = m->buckets[bkt];
    while (cur) {
        if (cur->key == key) return cur;
        cur = cur->next;
    }
    return NULL;
}

/* Reorganize: when load > 0.75, double bucket count + rehash.
 * We rebuild rather than incremental-rehash — bounded sizes here
 * keep it cheap. */
static void weakmap_grow(WeakMap *m) {
    uint32_t old_n = m->n_buckets;
    WeakMapEntry **old = m->buckets;
    uint32_t new_n = old_n * 2;
    WeakMapEntry **next_buckets = (WeakMapEntry **)calloc(new_n, sizeof(WeakMapEntry *));
    for (uint32_t i = 0; i < old_n; i++) {
        WeakMapEntry *cur = old[i];
        while (cur) {
            WeakMapEntry *next = cur->next;
            uint32_t bkt = hash_ptr_for(cur->key, new_n);
            cur->next = next_buckets[bkt];
            next_buckets[bkt] = cur;
            cur = next;
        }
    }
    free(old);
    m->buckets = next_buckets;
    m->n_buckets = new_n;
}

/* ============================================================
 * Public API.
 * ============================================================ */

void *__torajs_weakmap_create(void) {
    WeakMap *m = (WeakMap *)malloc(sizeof(WeakMap));
    m->header.refcount = 1;
    m->header.type_tag = __TORAJS_TAG_WEAKMAP;
    m->header.flags = 0;
    m->n_buckets = WEAKMAP_INITIAL_BUCKETS;
    m->n_entries = 0;
    m->buckets = (WeakMapEntry **)calloc(m->n_buckets, sizeof(WeakMapEntry *));
    return m;
}

/* `m.set(key, value)`. Replaces any existing entry for `key`
 * (dropping the old value's strong ref before installing the
 * new one). value gets rc_inc'd as it joins the map. NULL key
 * is rejected (per spec — WeakMap key must be an object) by
 * the typechecker, but we no-op here for defense. */
void __torajs_weakmap_set(void *p, void *key, void *value) {
    if (!p || !key) return;
    WeakMap *m = (WeakMap *)p;
    if ((m->n_entries + 1) * 4 > m->n_buckets * 3) {
        weakmap_grow(m);
    }
    uint32_t bkt = hash_ptr_for(key, m->n_buckets);
    WeakMapEntry *existing = weakmap_find(m, key, bkt);
    if (existing) {
        if (existing->value != NULL) {
            __torajs_value_drop_heap(existing->value);
        }
        if (value != NULL) __torajs_rc_inc(value);
        existing->value = value;
        return;
    }
    WeakMapEntry *e = (WeakMapEntry *)malloc(sizeof(WeakMapEntry));
    e->key = key;
    if (value != NULL) __torajs_rc_inc(value);
    e->value = value;
    e->next = m->buckets[bkt];
    m->buckets[bkt] = e;
    m->n_entries += 1;
    /* Register so weakref_target_dying can invalidate when the
     * key dies. The owner is the map ptr; the registry walks
     * the entry chain to find this exact key. */
    __torajs_weakref_registry_register(key, OBSERVER_WEAKMAP, m);
}

/* `m.get(key)` — returns the value with rc_inc, or NULL when
 * absent. Caller assumes ownership of the returned ref. */
void *__torajs_weakmap_get(void *p, void *key) {
    if (!p || !key) return NULL;
    WeakMap *m = (WeakMap *)p;
    uint32_t bkt = hash_ptr_for(key, m->n_buckets);
    WeakMapEntry *e = weakmap_find(m, key, bkt);
    if (!e) return NULL;
    if (e->value != NULL) __torajs_rc_inc(e->value);
    return e->value;
}

/* `m.has(key)` — returns 1 / 0 as i64 (since SSA-side bool widens
 * to i64 in print / arith paths). */
int64_t __torajs_weakmap_has(void *p, void *key) {
    if (!p || !key) return 0;
    WeakMap *m = (WeakMap *)p;
    uint32_t bkt = hash_ptr_for(key, m->n_buckets);
    return weakmap_find(m, key, bkt) ? 1 : 0;
}

/* `m.delete(key)` — returns 1 if key was present, 0 otherwise.
 * Drops the value's strong ref + frees the entry; deregisters
 * from the weakref registry so dying-key broadcasts skip this
 * (now-empty) target. */
int64_t __torajs_weakmap_delete(void *p, void *key) {
    if (!p || !key) return 0;
    WeakMap *m = (WeakMap *)p;
    uint32_t bkt = hash_ptr_for(key, m->n_buckets);
    WeakMapEntry **slot = &m->buckets[bkt];
    while (*slot) {
        if ((*slot)->key == key) {
            WeakMapEntry *gone = *slot;
            *slot = gone->next;
            if (gone->value != NULL) {
                __torajs_value_drop_heap(gone->value);
            }
            free(gone);
            m->n_entries -= 1;
            __torajs_weakref_registry_deregister(key, OBSERVER_WEAKMAP, m);
            return 1;
        }
        slot = &(*slot)->next;
    }
    return 0;
}

/* Called by the shared registry when `dying_key` (a WeakMap-
 * registered key) is about to free. We remove the entry without
 * touching the registry (the cell is being torn down by the
 * caller anyway) and drop the value. */
void __torajs_weakmap_invalidate_key(void *p, void *dying_key) {
    if (!p || !dying_key) return;
    WeakMap *m = (WeakMap *)p;
    uint32_t bkt = hash_ptr_for(dying_key, m->n_buckets);
    WeakMapEntry **slot = &m->buckets[bkt];
    while (*slot) {
        if ((*slot)->key == dying_key) {
            WeakMapEntry *gone = *slot;
            *slot = gone->next;
            if (gone->value != NULL) {
                __torajs_value_drop_heap(gone->value);
            }
            free(gone);
            m->n_entries -= 1;
            return;
        }
        slot = &(*slot)->next;
    }
}

/* rc-aware drop. Called from value_drop_heap's TAG_WEAKMAP case.
 * On last owner: walk every entry, drop its value's strong ref +
 * deregister from the weakref registry, free the entry, free the
 * bucket array, free the map. */
void __torajs_weakmap_drop(void *p) {
    if (!p) return;
    WeakMap *m = (WeakMap *)p;
    if (m->header.flags & 4 /* STATIC_LITERAL */) return;
    m->header.refcount -= 1;
    if (m->header.refcount != 0) return;
    for (uint32_t i = 0; i < m->n_buckets; i++) {
        WeakMapEntry *cur = m->buckets[i];
        while (cur) {
            WeakMapEntry *next = cur->next;
            if (cur->value != NULL) {
                __torajs_value_drop_heap(cur->value);
            }
            __torajs_weakref_registry_deregister(cur->key, OBSERVER_WEAKMAP, m);
            free(cur);
            cur = next;
        }
    }
    free(m->buckets);
    free(m);
}
