/*
 * runtime_weakset.c — torajs T-26.B (v0.7) WeakSet.
 *
 * Like WeakMap but with no value side — entries hold only the key.
 * Add, has, delete are O(1) average. Each entry registers in the
 * shared weakref registry under the key; on key death the
 * registry's broadcast invokes __torajs_weakset_invalidate_key
 * to remove the entry.
 *
 * Layout mirrors WeakMap minus the value slot:
 *
 *     [universal_heap_header (8B)]
 *     [bucket count u32]
 *     [entry count u32]
 *     [bucket array ptr (8B)]   — buckets[N], each = WeakSetEntry*
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_WEAKSET 13

typedef enum {
    OBSERVER_WEAKREF = 0,
    OBSERVER_WEAKMAP = 1,
    OBSERVER_WEAKSET = 2,
} ObserverKind;
extern void __torajs_weakref_registry_register(void *target, ObserverKind kind, void *owner);
extern void __torajs_weakref_registry_deregister(void *target, ObserverKind kind, void *owner);

typedef struct WeakSetEntry {
    void *key;
    struct WeakSetEntry *next;
} WeakSetEntry;

typedef struct {
    __torajs_heap_header_t header;
    uint32_t n_buckets;
    uint32_t n_entries;
    WeakSetEntry **buckets;
} WeakSet;

#define WEAKSET_INITIAL_BUCKETS 16

static inline uint32_t hash_ptr_for(void *p, uint32_t n_buckets) {
    uintptr_t v = (uintptr_t)p;
    v ^= v >> 33; v *= 0xff51afd7ed558ccdULL;
    v ^= v >> 33; v *= 0xc4ceb9fe1a85ec53ULL;
    v ^= v >> 33;
    return (uint32_t)(v & (n_buckets - 1));
}

static WeakSetEntry *weakset_find(WeakSet *s, void *key, uint32_t bkt) {
    WeakSetEntry *cur = s->buckets[bkt];
    while (cur) {
        if (cur->key == key) return cur;
        cur = cur->next;
    }
    return NULL;
}

static void weakset_grow(WeakSet *s) {
    uint32_t old_n = s->n_buckets;
    WeakSetEntry **old = s->buckets;
    uint32_t new_n = old_n * 2;
    WeakSetEntry **next_buckets = (WeakSetEntry **)calloc(new_n, sizeof(WeakSetEntry *));
    for (uint32_t i = 0; i < old_n; i++) {
        WeakSetEntry *cur = old[i];
        while (cur) {
            WeakSetEntry *next = cur->next;
            uint32_t bkt = hash_ptr_for(cur->key, new_n);
            cur->next = next_buckets[bkt];
            next_buckets[bkt] = cur;
            cur = next;
        }
    }
    free(old);
    s->buckets = next_buckets;
    s->n_buckets = new_n;
}

void *__torajs_weakset_create(void) {
    WeakSet *s = (WeakSet *)malloc(sizeof(WeakSet));
    s->header.refcount = 1;
    s->header.type_tag = __TORAJS_TAG_WEAKSET;
    s->header.flags = 0;
    s->n_buckets = WEAKSET_INITIAL_BUCKETS;
    s->n_entries = 0;
    s->buckets = (WeakSetEntry **)calloc(s->n_buckets, sizeof(WeakSetEntry *));
    return s;
}

void __torajs_weakset_add(void *p, void *key) {
    if (!p || !key) return;
    WeakSet *s = (WeakSet *)p;
    if ((s->n_entries + 1) * 4 > s->n_buckets * 3) {
        weakset_grow(s);
    }
    uint32_t bkt = hash_ptr_for(key, s->n_buckets);
    if (weakset_find(s, key, bkt)) return; /* idempotent */
    WeakSetEntry *e = (WeakSetEntry *)malloc(sizeof(WeakSetEntry));
    e->key = key;
    e->next = s->buckets[bkt];
    s->buckets[bkt] = e;
    s->n_entries += 1;
    __torajs_weakref_registry_register(key, OBSERVER_WEAKSET, s);
}

int64_t __torajs_weakset_has(void *p, void *key) {
    if (!p || !key) return 0;
    WeakSet *s = (WeakSet *)p;
    uint32_t bkt = hash_ptr_for(key, s->n_buckets);
    return weakset_find(s, key, bkt) ? 1 : 0;
}

int64_t __torajs_weakset_delete(void *p, void *key) {
    if (!p || !key) return 0;
    WeakSet *s = (WeakSet *)p;
    uint32_t bkt = hash_ptr_for(key, s->n_buckets);
    WeakSetEntry **slot = &s->buckets[bkt];
    while (*slot) {
        if ((*slot)->key == key) {
            WeakSetEntry *gone = *slot;
            *slot = gone->next;
            free(gone);
            s->n_entries -= 1;
            __torajs_weakref_registry_deregister(key, OBSERVER_WEAKSET, s);
            return 1;
        }
        slot = &(*slot)->next;
    }
    return 0;
}

void __torajs_weakset_invalidate_key(void *p, void *dying_key) {
    if (!p || !dying_key) return;
    WeakSet *s = (WeakSet *)p;
    uint32_t bkt = hash_ptr_for(dying_key, s->n_buckets);
    WeakSetEntry **slot = &s->buckets[bkt];
    while (*slot) {
        if ((*slot)->key == dying_key) {
            WeakSetEntry *gone = *slot;
            *slot = gone->next;
            free(gone);
            s->n_entries -= 1;
            return;
        }
        slot = &(*slot)->next;
    }
}

void __torajs_weakset_drop(void *p) {
    if (!p) return;
    WeakSet *s = (WeakSet *)p;
    if (s->header.flags & 4 /* STATIC_LITERAL */) return;
    s->header.refcount -= 1;
    if (s->header.refcount != 0) return;
    for (uint32_t i = 0; i < s->n_buckets; i++) {
        WeakSetEntry *cur = s->buckets[i];
        while (cur) {
            WeakSetEntry *next = cur->next;
            __torajs_weakref_registry_deregister(cur->key, OBSERVER_WEAKSET, s);
            free(cur);
            cur = next;
        }
    }
    free(s->buckets);
    free(s);
}
