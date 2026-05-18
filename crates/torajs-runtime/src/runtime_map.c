/*
 * runtime_map.c — torajs P6.1 strong-ref `Map<K, V>`.
 *
 * `new Map()` returns a heap struct holding an internal
 * open-addressing robin-hood hash table. Both keys and values are
 * stored as 16-byte tagged Any slots so `Map<any, any>` works
 * naturally; typed `Map<string, T>` shapes still share this storage
 * (the SSA side boxes / unboxes at the call boundary).
 *
 * Key equality follows SameValueZero (string byte-equal / IEEE-754
 * number with NaN == NaN / pointer identity for objects / arrays /
 * functions / closures / dates / regex / etc.). Per ES §23.1.3.1
 * Map.prototype.set, the "previous value" semantics apply: setting an
 * existing key replaces the value (the old value's strong ref is
 * dropped before the new ref is installed).
 *
 * Heap layout:
 *
 *     [universal_heap_header (8B)]
 *     [n_entries u32]      — live entries (excludes tombstones)
 *     [n_capacity u32]     — bucket array length; always power of 2
 *     [n_tombstones u32]
 *     [_pad u32]           — align
 *     [entries ptr (8B)]   — MapEntry[n_capacity]
 *
 * `MapEntry` is 40 bytes:
 *
 *     [hash u32]           — 0 = empty, 1 = tombstone; real hashes
 *                            force MSB so they're always >= 2
 *     [probe_dist u32]     — robin-hood probe distance from the
 *                            entry's "ideal" slot
 *     [key_tag u8 + 7 pad] — Any tag byte (TAG_INT / TAG_HEAP / …)
 *     [key_payload u64]
 *     [value_tag u8 + 7 pad]
 *     [value_payload u64]
 *
 * Initial capacity is 8; grow on load > 0.75. Tombstones are
 * cleaned out on every grow (no separate compaction pass).
 *
 * P6.1 Step 1 + 2 scope: heap struct + tag + alloc + drop. The
 * actual SameValueZero hash + lookup + set / get / has / delete
 * land in subsequent sub-steps (Step 3+). Drop walks every live
 * entry and drops both key + value via value_drop_heap (which
 * dispatches per-type for heap-tagged Any values; primitive-tagged
 * entries are no-ops).
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_MAP 6

/* ANY tag enum values shared with runtime_str.c. Only HEAP (4)
 * routes through value_drop_heap on drop; the inline-payload tags
 * (NULL/BOOL/I64/F64/UNDEF) are no-ops. */
#define __TORAJS_ANY_NULL    0
#define __TORAJS_ANY_BOOL    1
#define __TORAJS_ANY_I64     2
#define __TORAJS_ANY_F64     3
#define __TORAJS_ANY_HEAP    4
#define __TORAJS_ANY_UNDEF   5

#define __TORAJS_TAG_STR     0

#define __TORAJS_STR_HDR_SIZE 24
#define __TORAJS_STR_LEN(p)   (*(const uint64_t *)((const uint8_t *)(p) + 8))
#define __TORAJS_STR_CDATA(p) ((const uint8_t *)(p) + __TORAJS_STR_HDR_SIZE)

extern void __torajs_value_drop_heap(void *p);
extern void __torajs_rc_inc(void *p);
extern int64_t __torajs_str_eq(const uint8_t *a, const uint8_t *b);

/* MapEntry — see file header for layout. Total 40 bytes. */
typedef struct {
    uint32_t hash;
    uint32_t probe_dist;
    uint8_t  key_tag;
    uint8_t  _kpad[7];
    uint64_t key_payload;
    uint8_t  value_tag;
    uint8_t  _vpad[7];
    uint64_t value_payload;
} MapEntry;

typedef struct {
    __torajs_heap_header_t header;
    uint32_t n_entries;
    uint32_t n_capacity;
    uint32_t n_tombstones;
    uint32_t _pad;
    MapEntry *entries;
} Map;

#define MAP_INITIAL_CAPACITY 8u
#define MAP_HASH_EMPTY 0u
#define MAP_HASH_TOMBSTONE 1u

/* ============================================================
 * Public API — Step 1 + 2 surface (alloc + drop).
 * ============================================================ */

/* `new Map()` — alloc fresh empty map. capacity is power of 2
 * (8 initially). entries[] is calloc'd so every slot starts with
 * hash == MAP_HASH_EMPTY (== 0). */
void *__torajs_map_create(void) {
    Map *m = (Map *)malloc(sizeof(Map));
    m->header.refcount = 1;
    m->header.type_tag = __TORAJS_TAG_MAP;
    m->header.flags = 0;
    m->n_entries = 0;
    m->n_capacity = MAP_INITIAL_CAPACITY;
    m->n_tombstones = 0;
    m->_pad = 0;
    m->entries = (MapEntry *)calloc(m->n_capacity, sizeof(MapEntry));
    return m;
}

/* Drop a single entry's key + value rc-ref if the corresponding tag
 * carries the heap bit. Heap-tagged Any values point at an
 * rc-aware heap object whose drop is value_drop_heap (which
 * dispatches per-type via the universal heap header). Primitive-
 * tagged entries (int / f64 / bool / null / undef) are no-ops. */
static void map_entry_drop_refs(MapEntry *e) {
    if (e->key_tag == __TORAJS_ANY_HEAP) {
        void *kp = (void *)(uintptr_t)e->key_payload;
        if (kp != NULL) __torajs_value_drop_heap(kp);
    }
    if (e->value_tag == __TORAJS_ANY_HEAP) {
        void *vp = (void *)(uintptr_t)e->value_payload;
        if (vp != NULL) __torajs_value_drop_heap(vp);
    }
}

/* rc-aware drop. Called from value_drop_heap's TAG_MAP case (Step 5
 * wires this into the dispatch table). On last owner: walk every
 * live entry, drop both refs, free the entries array, free the
 * Map struct. STATIC_LITERAL flag (bit 2) is reserved for future
 * compile-time-known Maps; current Maps are always heap-fresh. */
void __torajs_map_drop(void *p) {
    if (!p) return;
    Map *m = (Map *)p;
    if (m->header.flags & 4 /* STATIC_LITERAL */) return;
    m->header.refcount -= 1;
    if (m->header.refcount != 0) return;
    for (uint32_t i = 0; i < m->n_capacity; i++) {
        MapEntry *e = &m->entries[i];
        if (e->hash == MAP_HASH_EMPTY || e->hash == MAP_HASH_TOMBSTONE) continue;
        map_entry_drop_refs(e);
    }
    free(m->entries);
    free(m);
}

/* `m.size` getter — exposed as a plain helper since SSA Member
 * access on a getter goes through this for now. Returns i64 to
 * match the typed-tier callsite (Number is i64-bit-wide at SSA). */
int64_t __torajs_map_size(void *p) {
    if (!p) return 0;
    Map *m = (Map *)p;
    return (int64_t)m->n_entries;
}

/* ============================================================
 * Hashing + key equality (SameValueZero, spec §7.2.10).
 * ============================================================ */

static inline uint32_t map_mix_u64(uint64_t x) {
    /* SplitMix64 finalizer — strong avalanche, used by V8 / Java /
     * many others as a mixing primitive. */
    x ^= x >> 33;
    x *= 0xff51afd7ed558ccdULL;
    x ^= x >> 33;
    x *= 0xc4ceb9fe1a85ec53ULL;
    x ^= x >> 33;
    return (uint32_t)x;
}

/* FNV-1a 64-bit over the byte slice, then SplitMix down to 32. */
static uint32_t map_hash_bytes(const uint8_t *bytes, uint64_t len) {
    uint64_t h = 0xcbf29ce484222325ULL;
    for (uint64_t i = 0; i < len; i++) {
        h ^= bytes[i];
        h *= 0x100000001b3ULL;
    }
    return map_mix_u64(h);
}

/* Compute the 32-bit hash for an Any-tagged key. The returned
 * value is always >= 2 — values 0 and 1 are reserved sentinels
 * (empty slot / tombstone) in the bucket array. */
static uint32_t map_hash_key(uint8_t tag, uint64_t payload) {
    uint32_t h;
    switch (tag) {
    case __TORAJS_ANY_NULL:
        h = 0xa5a5a5a5u;
        break;
    case __TORAJS_ANY_UNDEF:
        h = 0x5a5a5a5au;
        break;
    case __TORAJS_ANY_BOOL:
        h = map_mix_u64((uint64_t)payload ^ 0xb001ULL);
        break;
    case __TORAJS_ANY_I64:
        h = map_mix_u64(payload ^ 0xa11ULL);
        break;
    case __TORAJS_ANY_F64: {
        double d;
        memcpy(&d, &payload, sizeof(d));
        /* SameValueZero: NaN hashes to a canonical value so all
         * NaN bit patterns collide into the same bucket; +0/-0
         * share a hash (the payload bits differ but the value is
         * the same). */
        if (d != d) {
            h = 0xdeadbeefu;
        } else if (d == 0.0) {
            h = 0xfa57c0deu;
        } else {
            h = map_mix_u64(payload ^ 0xa11ULL);
        }
        break;
    }
    case __TORAJS_ANY_HEAP: {
        void *p = (void *)(uintptr_t)payload;
        if (p == NULL) {
            h = 0x12345678u;
            break;
        }
        __torajs_heap_header_t *hdr = (__torajs_heap_header_t *)p;
        if (hdr->type_tag == __TORAJS_TAG_STR) {
            h = map_hash_bytes(__TORAJS_STR_CDATA(p), __TORAJS_STR_LEN(p));
        } else {
            /* Pointer-identity hash for non-Str heap objects. */
            h = map_mix_u64((uint64_t)(uintptr_t)p ^ 0xf00dULL);
        }
        break;
    }
    default:
        h = map_mix_u64(payload ^ 0xbadULL);
        break;
    }
    if (h < 2) h = 2;
    return h;
}

/* SameValueZero comparison between two Any-tagged keys. Returns 1
 * if equal, 0 otherwise. NaN === NaN; +0 === -0; strings compare
 * byte-by-byte; non-Str heap objects compare by pointer identity. */
static int map_keys_equal(uint8_t ta, uint64_t pa, uint8_t tb, uint64_t pb) {
    if (ta != tb) return 0;
    switch (ta) {
    case __TORAJS_ANY_NULL:
    case __TORAJS_ANY_UNDEF:
        return 1;
    case __TORAJS_ANY_BOOL:
    case __TORAJS_ANY_I64:
        return pa == pb ? 1 : 0;
    case __TORAJS_ANY_F64: {
        double da, db;
        memcpy(&da, &pa, sizeof(da));
        memcpy(&db, &pb, sizeof(db));
        if (da != da) return db != db ? 1 : 0;
        if (db != db) return 0;
        return da == db ? 1 : 0;   /* +0 == -0 holds under IEEE eq */
    }
    case __TORAJS_ANY_HEAP: {
        void *pa_p = (void *)(uintptr_t)pa;
        void *pb_p = (void *)(uintptr_t)pb;
        if (pa_p == pb_p) return 1;
        if (pa_p == NULL || pb_p == NULL) return 0;
        __torajs_heap_header_t *ha = (__torajs_heap_header_t *)pa_p;
        __torajs_heap_header_t *hb = (__torajs_heap_header_t *)pb_p;
        if (ha->type_tag != hb->type_tag) return 0;
        if (ha->type_tag == __TORAJS_TAG_STR) {
            return (int)__torajs_str_eq((const uint8_t *)pa_p, (const uint8_t *)pb_p);
        }
        return 0;  /* identity already checked above */
    }
    default:
        return pa == pb ? 1 : 0;
    }
}

/* ============================================================
 * Open-addressing robin-hood probing.
 *
 * Each slot stores its full hash + probe distance. On insert we
 * walk the probe sequence comparing probe distances; when we
 * encounter a slot whose probe distance is *less* than ours, we
 * displace it (taking its slot, then continuing to insert the
 * displaced entry). This keeps the variance in probe length low,
 * which is what gives robin-hood its predictable performance.
 *
 * Lookup walks the same sequence but stops as soon as probe
 * distance exceeds the expected distance for the searched key —
 * if the key were present, it would have displaced an entry at
 * that point.
 *
 * Tombstones (hash == 1) participate in probing (skip-and-continue)
 * but are reclaimed on grow. We rehash on (load + tombstones) >
 * 0.75 × capacity.
 * ============================================================ */

#define MAP_LOAD_NUMER 3
#define MAP_LOAD_DENOM 4

static void map_grow(Map *m, uint32_t new_cap);

/* Insert an entry into a freshly-zeroed buckets array; assumes no
 * resize is needed (caller has sized the new array). Used by
 * `map_grow`'s rehash loop. */
static void map_insert_raw(MapEntry *buckets, uint32_t cap,
                           uint32_t hash,
                           uint8_t kt, uint64_t kp,
                           uint8_t vt, uint64_t vp) {
    uint32_t mask = cap - 1;
    uint32_t i = hash & mask;
    uint32_t probe = 0;
    uint8_t ct_kt = kt, ct_vt = vt;
    uint64_t ct_kp = kp, ct_vp = vp;
    uint32_t ct_hash = hash;
    for (;;) {
        MapEntry *e = &buckets[i];
        if (e->hash == MAP_HASH_EMPTY) {
            e->hash = ct_hash;
            e->probe_dist = probe;
            e->key_tag = ct_kt;
            e->key_payload = ct_kp;
            e->value_tag = ct_vt;
            e->value_payload = ct_vp;
            return;
        }
        if (e->probe_dist < probe) {
            /* Robin-hood: displace this richer entry. */
            uint32_t nh = e->hash; uint32_t np = e->probe_dist;
            uint8_t nkt = e->key_tag; uint64_t nkp = e->key_payload;
            uint8_t nvt = e->value_tag; uint64_t nvp = e->value_payload;
            e->hash = ct_hash; e->probe_dist = probe;
            e->key_tag = ct_kt; e->key_payload = ct_kp;
            e->value_tag = ct_vt; e->value_payload = ct_vp;
            ct_hash = nh; probe = np;
            ct_kt = nkt; ct_kp = nkp;
            ct_vt = nvt; ct_vp = nvp;
        }
        i = (i + 1) & mask;
        probe += 1;
    }
}

static void map_grow(Map *m, uint32_t new_cap) {
    MapEntry *old = m->entries;
    uint32_t old_cap = m->n_capacity;
    MapEntry *next = (MapEntry *)calloc(new_cap, sizeof(MapEntry));
    for (uint32_t i = 0; i < old_cap; i++) {
        MapEntry *e = &old[i];
        if (e->hash == MAP_HASH_EMPTY || e->hash == MAP_HASH_TOMBSTONE) continue;
        map_insert_raw(next, new_cap, e->hash,
                       e->key_tag, e->key_payload,
                       e->value_tag, e->value_payload);
    }
    free(old);
    m->entries = next;
    m->n_capacity = new_cap;
    m->n_tombstones = 0;
}

/* Locate the bucket index for `key` if present. Returns capacity
 * (out-of-range) when not found. On match, also fills *out_hash. */
static uint32_t map_find_slot(const Map *m, uint8_t kt, uint64_t kp,
                              uint32_t *out_hash) {
    uint32_t hash = map_hash_key(kt, kp);
    uint32_t mask = m->n_capacity - 1;
    uint32_t i = hash & mask;
    uint32_t probe = 0;
    for (;;) {
        MapEntry *e = &m->entries[i];
        if (e->hash == MAP_HASH_EMPTY) {
            *out_hash = hash;
            return m->n_capacity;
        }
        if (e->hash != MAP_HASH_TOMBSTONE && e->hash == hash
            && map_keys_equal(e->key_tag, e->key_payload, kt, kp)) {
            *out_hash = hash;
            return i;
        }
        if (e->hash != MAP_HASH_TOMBSTONE && e->probe_dist < probe) {
            /* Past where the key would have been placed. */
            *out_hash = hash;
            return m->n_capacity;
        }
        i = (i + 1) & mask;
        probe += 1;
        if (probe >= m->n_capacity) {
            *out_hash = hash;
            return m->n_capacity;
        }
    }
}

/* ============================================================
 * Public API — Step 3 + 4 surface (set / get / has / delete / clear).
 *
 * Convention: the SSA-side caller passes Any-tagged (tag, payload)
 * pairs already unboxed. For HEAP-tagged operands the caller has
 * already done one `rc_inc` (via `box_to_tag_value` /
 * `any_payload_rc_inc`); the entry adopts that owning ref. The
 * matching `rc_dec` lives in `map_entry_drop_refs` (delete /
 * tombstone) and in the overwrite branch of `__torajs_map_set`.
 * This mirrors `__torajs_dynobj_set`'s ownership contract so the
 * SSA side has a single inc-once-then-transfer pattern across both
 * collections.
 * ============================================================ */

void __torajs_map_set(void *p,
                      int64_t key_tag, int64_t key_payload,
                      int64_t value_tag, int64_t value_payload) {
    if (!p) return;
    Map *m = (Map *)p;
    /* Reserve room first; grow if (entries + tombstones + 1) crosses
     * 0.75 × capacity. */
    if ((m->n_entries + m->n_tombstones + 1) * MAP_LOAD_DENOM
        > m->n_capacity * MAP_LOAD_NUMER) {
        map_grow(m, m->n_capacity * 2);
    }
    uint8_t kt = (uint8_t)key_tag, vt = (uint8_t)value_tag;
    uint64_t kp = (uint64_t)key_payload, vp = (uint64_t)value_payload;
    uint32_t hash;
    uint32_t slot = map_find_slot(m, kt, kp, &hash);
    if (slot < m->n_capacity) {
        /* Existing entry — drop old value, install new. Caller has
         * already rc_inc'd the new value (per the ownership
         * contract); the old key stays in the table so the caller's
         * key rc bump must be released to keep the count balanced. */
        MapEntry *e = &m->entries[slot];
        if (e->value_tag == __TORAJS_ANY_HEAP) {
            void *old_vp = (void *)(uintptr_t)e->value_payload;
            if (old_vp != NULL) __torajs_value_drop_heap(old_vp);
        }
        if (kt == __TORAJS_ANY_HEAP) {
            void *new_kp = (void *)(uintptr_t)kp;
            if (new_kp != NULL) __torajs_value_drop_heap(new_kp);
        }
        e->value_tag = vt;
        e->value_payload = vp;
        return;
    }
    /* Fresh insert — caller's rc_inc on heap key + heap value is
     * adopted directly into the entry's slot. */
    map_insert_raw(m->entries, m->n_capacity, hash, kt, kp, vt, vp);
    m->n_entries += 1;
}

/* Per-call helper used by every query-path helper: query borrows
 * the caller's rc bump on a heap-tagged key, so we release it
 * before returning. Set / fresh-insert keep the bump (the entry
 * adopts it); set-on-existing also drops since the original key
 * stays in the table. */
static void map_drop_borrowed_key(uint8_t tag, uint64_t payload) {
    if (tag == __TORAJS_ANY_HEAP) {
        void *kp = (void *)(uintptr_t)payload;
        if (kp != NULL) __torajs_value_drop_heap(kp);
    }
}

/* Returns 1 / 0 (boolean-shaped) per `m.has(k)` spec §23.1.3.7. */
int64_t __torajs_map_has(void *p, int64_t key_tag, int64_t key_payload) {
    int64_t r = 0;
    if (p) {
        Map *m = (Map *)p;
        uint32_t hash;
        uint32_t slot = map_find_slot(m, (uint8_t)key_tag, (uint64_t)key_payload, &hash);
        r = slot < m->n_capacity ? 1 : 0;
    }
    map_drop_borrowed_key((uint8_t)key_tag, (uint64_t)key_payload);
    return r;
}

/* `m.get(k)` — fills out_tag / out_payload with the stored value
 * (rc_inc'd when heap) or with ANY_UNDEF / 0 when absent. The
 * caller owns the returned ref. Two-output convention via out
 * pointers avoids a temp Any-box alloc on the common hit path. */
void __torajs_map_get(void *p, int64_t key_tag, int64_t key_payload,
                      int64_t *out_tag, int64_t *out_payload) {
    if (!p) {
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        map_drop_borrowed_key((uint8_t)key_tag, (uint64_t)key_payload);
        return;
    }
    Map *m = (Map *)p;
    uint32_t hash;
    uint32_t slot = map_find_slot(m, (uint8_t)key_tag, (uint64_t)key_payload, &hash);
    if (slot >= m->n_capacity) {
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        map_drop_borrowed_key((uint8_t)key_tag, (uint64_t)key_payload);
        return;
    }
    MapEntry *e = &m->entries[slot];
    *out_tag = (int64_t)e->value_tag;
    *out_payload = (int64_t)e->value_payload;
    if (e->value_tag == __TORAJS_ANY_HEAP) {
        void *vp = (void *)(uintptr_t)e->value_payload;
        if (vp != NULL) __torajs_rc_inc(vp);
    }
    map_drop_borrowed_key((uint8_t)key_tag, (uint64_t)key_payload);
}

/* `m.delete(k)` — returns 1 if key was present, 0 otherwise. The
 * entry's key + value refs are dropped; the slot becomes a
 * tombstone (cleaned up on next grow). */
int64_t __torajs_map_delete(void *p, int64_t key_tag, int64_t key_payload) {
    int64_t r = 0;
    if (p) {
        Map *m = (Map *)p;
        uint32_t hash;
        uint32_t slot = map_find_slot(m, (uint8_t)key_tag, (uint64_t)key_payload, &hash);
        if (slot < m->n_capacity) {
            MapEntry *e = &m->entries[slot];
            map_entry_drop_refs(e);
            e->hash = MAP_HASH_TOMBSTONE;
            e->probe_dist = 0;
            e->key_tag = 0;
            e->key_payload = 0;
            e->value_tag = 0;
            e->value_payload = 0;
            m->n_entries -= 1;
            m->n_tombstones += 1;
            if (m->n_tombstones > m->n_capacity / 4) {
                map_grow(m, m->n_capacity);
            }
            r = 1;
        }
    }
    map_drop_borrowed_key((uint8_t)key_tag, (uint64_t)key_payload);
    return r;
}

/* `m.clear()` — drop all entries; keep the buckets array (reset to
 * empty so subsequent inserts skip the initial allocation). */
void __torajs_map_clear(void *p) {
    if (!p) return;
    Map *m = (Map *)p;
    for (uint32_t i = 0; i < m->n_capacity; i++) {
        MapEntry *e = &m->entries[i];
        if (e->hash == MAP_HASH_EMPTY || e->hash == MAP_HASH_TOMBSTONE) continue;
        map_entry_drop_refs(e);
    }
    memset(m->entries, 0, (size_t)m->n_capacity * sizeof(MapEntry));
    m->n_entries = 0;
    m->n_tombstones = 0;
}
