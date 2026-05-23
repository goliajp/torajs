/*
 * runtime_map.c — torajs P6.1 strong-ref `Map<K, V>`.
 *
 * `new Map()` returns a heap struct holding an internal split-table
 * hash map (V8 OrderedHashMap / Python dict shape): a separate
 * `entries[]` array packs every (k, v) in insertion order, while a
 * `slots[]` array uses open-addressing robin-hood probing to map a
 * hash to an entry-index. Iteration walks `entries[]` in order, so
 * `m.forEach / .entries / .keys / .values` see spec-mandated
 * insertion-order semantics (§23.1.4) — no auxiliary linked list
 * needed, no entry shuffling on robin-hood displacement (slots
 * move; entries stay put).
 *
 * Both keys and values are stored as 16-byte tagged Any slots so
 * `Map<any, any>` works naturally; typed `Map<string, T>` shapes
 * still share this storage (the SSA side boxes / unboxes at the
 * call boundary).
 *
 * Key equality follows SameValueZero (string byte-equal / IEEE-754
 * number with NaN == NaN / pointer identity for objects / arrays /
 * functions / closures / dates / regex / etc.). Per ES §23.1.3.1
 * Map.prototype.set, the "previous value" semantics apply: setting
 * an existing key replaces the value (the old value's strong ref is
 * dropped before the new ref is installed).
 *
 * Heap layout:
 *
 *     [universal_heap_header (8B)]
 *     [n_entries u32]        — live entries (excludes tombstones)
 *     [n_used u32]            — entries used incl. tombstones (≤ entries_cap)
 *     [entries_cap u32]
 *     [slots_count u32]       — bucket array length; always power of 2
 *     [n_tombstones u32]      — slot-side tombstone count
 *     [_pad u32]              — align
 *     [slots ptr (8B)]        — u32[slots_count]; each = entry_index or sentinel
 *     [entries ptr (8B)]      — MapEntry[entries_cap], packed insertion-order
 *
 * Each slot stores an entry-index plus its real hash so robin-hood
 * probing can compare without dereferencing the entry. The slot
 * sentinels are `SLOT_EMPTY = UINT32_MAX` and `SLOT_TOMBSTONE =
 * UINT32_MAX - 1`. The slot at index `i` is a 64-bit pair
 * `(hash u32 << 32) | entry_index u32`; entry_index UINT32_MAX
 * marks empty, UINT32_MAX-1 marks a tombstone.
 *
 * Each `MapEntry` is 40 bytes:
 *
 *     [hash u32]              — 0 = tombstone (live entries always >= 1)
 *     [_pad u32]
 *     [key_tag u8 + 7 pad]
 *     [key_payload u64]
 *     [value_tag u8 + 7 pad]
 *     [value_payload u64]
 *
 * P6.1 + 2 + 3 ship: heap struct + tag + alloc + drop + SameValueZero
 * key hash / equality + set + get + has + delete + clear + size.
 * P6.4a adds the insertion-order iter substrate (entries[]-walk via
 * `__torajs_map_iter_next`) which forEach / entries / keys / values
 * consume.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

/* P6.1 — must NOT collide with the dispatch table in runtime_str.c
 * (TAG 0-7 already used: STR/OBJ/ARR/CLOSURE/REGEX/DATE/ANY_BOX/
 * SYMBOL; 8=Promise; 9-14 used by Response/BigInt/WeakRef/
 * WeakMap/WeakSet/DYNOBJ). Tag 15 is the first free slot. */
#define __TORAJS_TAG_MAP 15

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

/* Latent-bug fix (P4.3-b, 2026-05-23): __TORAJS_STR_HDR_SIZE was 24
 * here while the canonical Str layout (runtime_str.c + torajs-str)
 * is 16. C-side map_hash_key was reading 8 bytes past the actual
 * string payload — self-consistent (both `set` and `has` read the
 * same garbage) so bun-parity passed historically by luck of
 * identical garbage. Now Rust-side has/get reads the correct offset
 * 16; aligning C makes both tiers hash identically against the real
 * string bytes. */
#define __TORAJS_STR_HDR_SIZE 16
#define __TORAJS_STR_LEN(p)   (*(const uint64_t *)((const uint8_t *)(p) + 8))
#define __TORAJS_STR_CDATA(p) ((const uint8_t *)(p) + __TORAJS_STR_HDR_SIZE)

extern void __torajs_value_drop_heap(void *p);
extern void __torajs_rc_inc(void *p);
extern int64_t __torajs_str_eq(const uint8_t *a, const uint8_t *b);

/* P6.4c — `[k, v]` per-step Array<Any> for ENTRIES iter kind. */
extern void *__torajs_arr_alloc_any(uint64_t cap);
extern void *__torajs_arr_push_any(void *arr, uint64_t tag, uint64_t value);

/* Entry tombstone marker — a slot still pointing here lets iter
 * detect-and-skip without consulting the slot array. Live entries
 * always have `entry.hash >= 1` (the bucket-side `hash` mixes in
 * payload bits + an explicit `| 1` so it never lands on 0). */
#define ENTRY_HASH_TOMBSTONE 0u

/* Slot sentinels. Stored in the low 32 bits of the slot's 64-bit
 * cell; the high 32 bits hold the real hash for compare-without-
 * deref. */
#define SLOT_EMPTY     0xFFFFFFFFu
#define SLOT_TOMBSTONE 0xFFFFFFFEu

#define MAP_ENTRIES_INITIAL 8u
#define MAP_SLOTS_INITIAL   16u

/* Each slot in the open-addressing table is a 64-bit pair packing
 * the real hash (high 32) + the entry index into entries[] (low 32).
 * Compare without dereferencing entries[]. */
typedef uint64_t MapSlot;

#define SLOT_HASH(s)       ((uint32_t)((s) >> 32))
#define SLOT_INDEX(s)      ((uint32_t)((s) & 0xFFFFFFFFu))
#define SLOT_MAKE(h, idx)  ((((uint64_t)(h)) << 32) | (uint32_t)(idx))

typedef struct {
    uint32_t hash;
    uint32_t _pad;
    uint8_t  key_tag;
    uint8_t  _kpad[7];
    uint64_t key_payload;
    uint8_t  value_tag;
    uint8_t  _vpad[7];
    uint64_t value_payload;
} MapEntry;

typedef struct {
    __torajs_heap_header_t header;
    uint32_t n_entries;       /* live entry count */
    uint32_t n_used;          /* entries[] occupied prefix (incl. tombstones) */
    uint32_t entries_cap;
    uint32_t slots_count;
    uint32_t n_tombstones;    /* slot-side tombstones */
    uint32_t _pad;
    MapSlot *slots;
    MapEntry *entries;
} Map;

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
 * value is always >= 1 — value 0 is reserved as the entry-side
 * tombstone marker. */
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
    if (h == 0) h = 1;
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
 * Slot table — robin-hood probing over (hash, entry_index) pairs.
 *
 * The entries[] array is the source of truth for `m.set / get / has
 * / delete`; the slots[] table is purely an acceleration index.
 * Robin-hood swaps move SLOT cells around, never the underlying
 * MapEntry — entry indices are stable for the entry's lifetime,
 * so iteration order (which walks entries[]) is preserved no
 * matter how much the hash table reshuffles.
 * ============================================================ */

#define MAP_LOAD_NUMER 3
#define MAP_LOAD_DENOM 4

/* Insert into a freshly-zeroed slots array. Robin-hood: when the
 * incoming slot's probe distance exceeds the slot it visits, swap
 * and continue with the displaced cell. Caller is responsible for
 * having sized the array to accommodate the load. */
static void map_slot_insert(MapSlot *slots, uint32_t cap, uint32_t hash, uint32_t idx) {
    uint32_t mask = cap - 1;
    uint32_t i = hash & mask;
    uint32_t probe = 0;
    uint32_t cur_hash = hash;
    uint32_t cur_idx = idx;
    for (;;) {
        MapSlot s = slots[i];
        if (SLOT_INDEX(s) == SLOT_EMPTY) {
            slots[i] = SLOT_MAKE(cur_hash, cur_idx);
            return;
        }
        /* Probe distance = how far this slot is from its ideal. */
        uint32_t slot_ideal = SLOT_HASH(s) & mask;
        uint32_t slot_probe = (i + cap - slot_ideal) & mask;
        if (slot_probe < probe) {
            /* Robin-hood: displace this richer slot. */
            uint32_t old_hash = SLOT_HASH(s);
            uint32_t old_idx = SLOT_INDEX(s);
            slots[i] = SLOT_MAKE(cur_hash, cur_idx);
            cur_hash = old_hash;
            cur_idx = old_idx;
            probe = slot_probe;
        }
        i = (i + 1) & mask;
        probe += 1;
    }
}

/* Look up the entry index for `(tag, payload)`. Returns the slot
 * index (suitable for tombstone-marking on delete) and entry index
 * via out-params. Returns 1 on hit; on miss returns 0 and the slot
 * index where insertion would happen + hash. */
static int map_lookup_slot(const Map *m, uint8_t tag, uint64_t payload,
                           uint32_t *out_hash, uint32_t *out_slot_idx,
                           uint32_t *out_entry_idx) {
    uint32_t hash = map_hash_key(tag, payload);
    *out_hash = hash;
    uint32_t mask = m->slots_count - 1;
    uint32_t i = hash & mask;
    uint32_t probe = 0;
    uint32_t first_tomb = m->slots_count;
    for (;;) {
        MapSlot s = m->slots[i];
        uint32_t s_idx = SLOT_INDEX(s);
        if (s_idx == SLOT_EMPTY) {
            *out_slot_idx = (first_tomb != m->slots_count) ? first_tomb : i;
            *out_entry_idx = SLOT_EMPTY;
            return 0;
        }
        if (s_idx == SLOT_TOMBSTONE) {
            if (first_tomb == m->slots_count) first_tomb = i;
        } else if (SLOT_HASH(s) == hash) {
            MapEntry *e = &m->entries[s_idx];
            if (map_keys_equal(e->key_tag, e->key_payload, tag, payload)) {
                *out_slot_idx = i;
                *out_entry_idx = s_idx;
                return 1;
            }
        } else {
            /* Robin-hood early termination: if we've outprobed the
             * resident here and it's not a tombstone, the key
             * isn't anywhere later in the chain either. */
            uint32_t slot_ideal = SLOT_HASH(s) & mask;
            uint32_t slot_probe = (i + m->slots_count - slot_ideal) & mask;
            if (slot_probe < probe) {
                *out_slot_idx = (first_tomb != m->slots_count) ? first_tomb : i;
                *out_entry_idx = SLOT_EMPTY;
                return 0;
            }
        }
        i = (i + 1) & mask;
        probe += 1;
        if (probe >= m->slots_count) {
            *out_slot_idx = (first_tomb != m->slots_count) ? first_tomb : i;
            *out_entry_idx = SLOT_EMPTY;
            return 0;
        }
    }
}

/* Rebuild after deletes have accumulated tombstones in entries[],
 * after the entries[] array runs out of room, or after the slot
 * table crosses its load threshold. Compacts entries[] (removing
 * tombstones, preserving insertion order) into a fresh array of
 * `new_entries_cap` slots, and reinserts into a fresh slots[] of
 * `new_slots_count` capacity. */
static void map_rehash(Map *m, uint32_t new_entries_cap, uint32_t new_slots_count) {
    MapEntry *old_e = m->entries;
    uint32_t old_used = m->n_used;
    MapEntry *new_e = (MapEntry *)calloc(new_entries_cap, sizeof(MapEntry));
    MapSlot *new_s = (MapSlot *)malloc(new_slots_count * sizeof(MapSlot));
    for (uint32_t k = 0; k < new_slots_count; k++) new_s[k] = SLOT_MAKE(0, SLOT_EMPTY);
    uint32_t new_used = 0;
    for (uint32_t k = 0; k < old_used; k++) {
        MapEntry *src = &old_e[k];
        if (src->hash == ENTRY_HASH_TOMBSTONE) continue;
        new_e[new_used] = *src;
        map_slot_insert(new_s, new_slots_count, src->hash, new_used);
        new_used += 1;
    }
    free(old_e);
    free(m->slots);
    m->entries = new_e;
    m->slots = new_s;
    m->entries_cap = new_entries_cap;
    m->slots_count = new_slots_count;
    m->n_used = new_used;
    m->n_tombstones = 0;
}

/* ============================================================
 * Public API.
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

/* __torajs_map_create moved to torajs-collections::create (P4.3-a,
 * 2026-05-23). Symbol resolves cross-tier at `tr build` link time via
 * libtorajs_collections.a. No in-file C caller — no extern decl needed.
 * Successive sub-steps (P4.3-b..-g) port the rest of the Map / Set
 * surface; P4.3-h does MapIter; P4.3-i lifts ArrIter to torajs-arr
 * (it was misplaced here when MapIter was added in P6.4). */

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

/* rc-aware drop. Called from value_drop_heap's TAG_MAP case (see
 * runtime_str.c). On last owner: walk every live entry, drop both
 * refs, free the entries + slots arrays, free the Map struct. */
void __torajs_map_drop(void *p) {
    if (!p) return;
    Map *m = (Map *)p;
    if (m->header.flags & 4 /* STATIC_LITERAL */) return;
    m->header.refcount -= 1;
    if (m->header.refcount != 0) return;
    for (uint32_t i = 0; i < m->n_used; i++) {
        MapEntry *e = &m->entries[i];
        if (e->hash == ENTRY_HASH_TOMBSTONE) continue;
        map_entry_drop_refs(e);
    }
    free(m->slots);
    free(m->entries);
    free(m);
}

/* __torajs_map_size moved to torajs-collections::query (P4.3-b, 2026-05-23). */

/* Per-call helper used by every query-path helper: query borrows
 * the caller's rc bump on a heap-tagged key, so we release it
 * before returning. Still in C — used by C-side set / delete /
 * clear paths that haven't ported yet (they keep their own copy
 * of the contract). */
static void map_drop_borrowed_key(uint8_t tag, uint64_t payload) {
    if (tag == __TORAJS_ANY_HEAP) {
        void *kp = (void *)(uintptr_t)payload;
        if (kp != NULL) __torajs_value_drop_heap(kp);
    }
}

/* __torajs_map_set moved to torajs-collections::mutate (P4.3-c, 2026-05-24).
 * Same slot-load + entries-cap rehash triggers, same robin-hood probing
 * placement, same key-rc transfer-vs-release semantics. */

/* __torajs_map_has + __torajs_map_get moved to torajs-collections::query
 * (P4.3-b, 2026-05-23). Same borrow-then-drop key ownership contract
 * preserved; get rc-bumps the returned heap value before fill-out.
 * C-side set / delete / clear / iter still here — use their own
 * `map_drop_borrowed_key` static helper for the same purpose. */

/* `m.delete(k)` — returns 1 if key was present, 0 otherwise. The
 * entry is converted to an entries[]-side tombstone (hash=0) and
 * the slot becomes SLOT_TOMBSTONE so probe chains step over it. */
int64_t __torajs_map_delete(void *p, int64_t key_tag, int64_t key_payload) {
    int64_t r = 0;
    if (p) {
        Map *m = (Map *)p;
        uint32_t hash, slot_idx, entry_idx;
        int hit = map_lookup_slot(m, (uint8_t)key_tag, (uint64_t)key_payload,
                                  &hash, &slot_idx, &entry_idx);
        if (hit) {
            MapEntry *e = &m->entries[entry_idx];
            map_entry_drop_refs(e);
            e->hash = ENTRY_HASH_TOMBSTONE;
            e->key_tag = 0;
            e->key_payload = 0;
            e->value_tag = 0;
            e->value_payload = 0;
            m->slots[slot_idx] = SLOT_MAKE(0, SLOT_TOMBSTONE);
            m->n_entries -= 1;
            m->n_tombstones += 1;
            if (m->n_tombstones > m->slots_count / 4) {
                /* Compact entries[] + reset tombstones. */
                map_rehash(m, m->entries_cap, m->slots_count);
            }
            r = 1;
        }
    }
    map_drop_borrowed_key((uint8_t)key_tag, (uint64_t)key_payload);
    return r;
}

/* `m.clear()` — drop all entries; reset the slot table to empty.
 * Reuses the existing slots / entries allocations. */
void __torajs_map_clear(void *p) {
    if (!p) return;
    Map *m = (Map *)p;
    for (uint32_t i = 0; i < m->n_used; i++) {
        MapEntry *e = &m->entries[i];
        if (e->hash == ENTRY_HASH_TOMBSTONE) continue;
        map_entry_drop_refs(e);
    }
    memset(m->entries, 0, (size_t)m->entries_cap * sizeof(MapEntry));
    for (uint32_t k = 0; k < m->slots_count; k++) m->slots[k] = SLOT_MAKE(0, SLOT_EMPTY);
    m->n_entries = 0;
    m->n_used = 0;
    m->n_tombstones = 0;
}

/* P6.4a — `m.forEach` / `s.forEach` iter step. Walks `entries[]` in
 * insertion order (spec §23.1.4 / §24.2.4). Cursor is an i64 stack
 * slot the SSA side stores `-1` into to mark first-call; the helper
 * resolves that to entry-index 0 and advances by entries[] index
 * each call. Returns 1 when a live entry is filled into out-params,
 * 0 when the cursor has run off the end (or the map is empty). */
int64_t __torajs_map_iter_next(void *p,
                               int64_t *cursor,
                               int64_t *out_k_tag, int64_t *out_k_payload,
                               int64_t *out_v_tag, int64_t *out_v_payload) {
    if (!p) return 0;
    Map *m = (Map *)p;
    int64_t c = *cursor;
    uint32_t i = (c == -1LL) ? 0u : (uint32_t)c;
    while (i < m->n_used) {
        MapEntry *e = &m->entries[i];
        i += 1;
        if (e->hash == ENTRY_HASH_TOMBSTONE) continue;
        *out_k_tag = (int64_t)e->key_tag;
        *out_k_payload = (int64_t)e->key_payload;
        *out_v_tag = (int64_t)e->value_tag;
        *out_v_payload = (int64_t)e->value_payload;
        *cursor = (int64_t)i;
        return 1;
    }
    *cursor = (int64_t)m->n_used;
    return 0;
}

/* ============================================================
 * P6.4b — MapIter: stateful iterator returned by `m.keys() /
 * .values() / .entries()`. Holds a strong ref to the source Map
 * (so the entries[] array stays live through the iteration) plus
 * a cursor + kind. The user-facing surface is `iter.next()`,
 * returning an `IteratorResult<T>` (the SSA side wraps the value
 * payload into an Any-box + builds the struct).
 *
 * Three kinds:
 *   - KEYS    — yield the key of each live entry
 *   - VALUES  — yield the value of each live entry
 *   - ENTRIES — yield `[key, value]` (a fresh Array<Any> per step;
 *               substrate landed in P6.4c, NOT this commit)
 *
 * MapIter occupies its own type tag so it routes through
 * `value_drop_heap` correctly when used as an Any-box payload.
 * Tag 16 — next free slot after MAP=15.
 * ============================================================ */

#define __TORAJS_TAG_MAP_ITER 16

#define MAP_ITER_KEYS        0
#define MAP_ITER_VALUES      1
#define MAP_ITER_ENTRIES     2  /* Map.entries — yield `[key, value]` Array<Any> */
#define MAP_ITER_SET_ENTRIES 3  /* Set.entries — yield `[key, key]` (Set spec
                                 * §24.2.3.6: callback's second arg = first) */

typedef struct {
    __torajs_heap_header_t header;
    Map *map;         /* strong-ref to source Map; rc_inc on create, rc_dec on drop */
    int64_t cursor;   /* entries[] index of the NEXT slot to inspect, or n_used when done */
    uint32_t kind;
    uint32_t _pad;
} MapIter;

static void *map_iter_create_with_kind(void *map_p, uint32_t kind) {
    MapIter *it = (MapIter *)malloc(sizeof(MapIter));
    it->header.refcount = 1;
    it->header.type_tag = __TORAJS_TAG_MAP_ITER;
    it->header.flags = 0;
    it->map = (Map *)map_p;
    it->cursor = 0;
    it->kind = kind;
    it->_pad = 0;
    /* Hold a strong ref to the source so iteration stays valid
     * even if the caller drops their binding mid-iter. */
    if (map_p != NULL) __torajs_rc_inc(map_p);
    return it;
}

void *__torajs_map_iter_create_keys(void *map_p) {
    return map_iter_create_with_kind(map_p, MAP_ITER_KEYS);
}

void *__torajs_map_iter_create_values(void *map_p) {
    return map_iter_create_with_kind(map_p, MAP_ITER_VALUES);
}

void *__torajs_map_iter_create_entries(void *map_p) {
    return map_iter_create_with_kind(map_p, MAP_ITER_ENTRIES);
}

void *__torajs_map_iter_create_set_entries(void *map_p) {
    return map_iter_create_with_kind(map_p, MAP_ITER_SET_ENTRIES);
}

/* P6.4c — alloc a fresh `[a, b]` Array<Any> from two tagged-pair
 * sources. The bucket entry's heap payload still owns its ref, so
 * we rc_inc each ANY_HEAP payload before pushing (push adopts the
 * pre-bumped ref). On return the array is at refcount=1 owned by
 * the caller — but the caller wraps the result via `__torajs_any_box`
 * (ANY_HEAP, arr) which itself rc_incs the payload (so the box
 * would observe refcount=2 with only one drop chain → leak). We
 * pre-decrement the array's refcount to 0 here so any_box's inc
 * leaves it at exactly 1 owner = the caller's IteratorResult.value
 * Any-box. Same idiom would apply to any "freshly minted heap"
 * value funneled through the `(tag, payload)` → any_box path. */
static void *map_iter_make_pair_arr(uint8_t t1, uint64_t p1,
                                    uint8_t t2, uint64_t p2) {
    void *arr = __torajs_arr_alloc_any(2);
    if (t1 == __TORAJS_ANY_HEAP && p1 != 0) {
        __torajs_rc_inc((void *)(uintptr_t)p1);
    }
    arr = __torajs_arr_push_any(arr, (uint64_t)t1, p1);
    if (t2 == __TORAJS_ANY_HEAP && p2 != 0) {
        __torajs_rc_inc((void *)(uintptr_t)p2);
    }
    arr = __torajs_arr_push_any(arr, (uint64_t)t2, p2);
    ((__torajs_heap_header_t *)arr)->refcount -= 1;
    return arr;
}

/* P6.4b iter step. Advances the cursor + fills the (out_tag,
 * out_payload) pair with the next iterated value per the iter's
 * kind. Returns 1 if a value was produced, 0 if the cursor has
 * run past the end. On hit, the (tag, payload) pair is delivered
 * WITHOUT an rc_inc on the heap payload — the caller wraps it via
 * __torajs_any_box (which rc_incs heap payloads itself), so the
 * box adopts ownership. This mirrors the contract used by
 * map_iter_next for forEach. */
int64_t __torajs_map_iter_step(void *iter_p,
                               int64_t *out_tag, int64_t *out_payload) {
    if (!iter_p) {
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        return 0;
    }
    MapIter *it = (MapIter *)iter_p;
    Map *m = it->map;
    if (!m) {
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        return 0;
    }
    uint32_t i = (uint32_t)it->cursor;
    while (i < m->n_used) {
        MapEntry *e = &m->entries[i];
        i += 1;
        if (e->hash == ENTRY_HASH_TOMBSTONE) continue;
        switch (it->kind) {
        case MAP_ITER_KEYS:
            *out_tag = (int64_t)e->key_tag;
            *out_payload = (int64_t)e->key_payload;
            break;
        case MAP_ITER_VALUES:
            *out_tag = (int64_t)e->value_tag;
            *out_payload = (int64_t)e->value_payload;
            break;
        case MAP_ITER_ENTRIES: {
            /* P6.4c — yield `[key, value]` as Array<Any>. */
            void *arr = map_iter_make_pair_arr(
                e->key_tag, e->key_payload,
                e->value_tag, e->value_payload);
            *out_tag = __TORAJS_ANY_HEAP;
            *out_payload = (int64_t)(uintptr_t)arr;
            break;
        }
        case MAP_ITER_SET_ENTRIES: {
            /* P6.4c — Set.entries yields `[key, key]` per spec
             * §24.2.3.6 (the storage value is ANY_UNDEF placeholder;
             * iteration exposes the element twice). */
            void *arr = map_iter_make_pair_arr(
                e->key_tag, e->key_payload,
                e->key_tag, e->key_payload);
            *out_tag = __TORAJS_ANY_HEAP;
            *out_payload = (int64_t)(uintptr_t)arr;
            break;
        }
        default:
            *out_tag = __TORAJS_ANY_UNDEF;
            *out_payload = 0;
            break;
        }
        it->cursor = (int64_t)i;
        return 1;
    }
    it->cursor = (int64_t)m->n_used;
    *out_tag = __TORAJS_ANY_UNDEF;
    *out_payload = 0;
    return 0;
}

/* rc-aware drop. Routes through value_drop_heap's TAG_MAP_ITER arm
 * (wired in runtime_str.c). Releases the strong ref on the source
 * Map + frees the iter struct itself. */
void __torajs_map_iter_drop(void *p) {
    if (!p) return;
    MapIter *it = (MapIter *)p;
    if (it->header.flags & 4 /* STATIC_LITERAL */) return;
    it->header.refcount -= 1;
    if (it->header.refcount != 0) return;
    if (it->map != NULL) __torajs_value_drop_heap(it->map);
    free(it);
}

/* ============================================================
 * P6.4c-C3 — ArrIter: stateful iterator returned by
 * `arr.keys() / .values() / .entries()` for Array<Any> sources.
 * Same shape as MapIter — distinct type tag (17) so the drop
 * dispatch + Type::ArrIter SSA-level distinction stay clean.
 * Restricted to Array<Any> for now; typed Array<T> for non-Any T
 * needs an elem-tag field + per-tag step path (P5.4 follow-up).
 *
 * Array<Any> internal layout (defined in runtime_str.c):
 *   header(8) + len u64 + cap u32 + head_offset u32 + slots[cap]
 * Each slot is 16 bytes: tag u64 + payload u64. Read via the
 * `any_slot_tag_` / `any_slot_val_` accessors from runtime_str.c —
 * here we don't need them directly because the SSA side rebuilds
 * the IteratorResult struct from the (tag, payload) pair we emit.
 * ============================================================ */

#define __TORAJS_TAG_ARR_ITER 17

#define ARR_ITER_KEYS    0
#define ARR_ITER_VALUES  1
#define ARR_ITER_ENTRIES 2

#define __TORAJS_ARR_HDR_SIZE 24
#define __TORAJS_ANY_SLOT_BYTES 16

typedef struct {
    __torajs_heap_header_t header;
    void *arr;        /* strong-ref to source Array<Any> */
    int64_t cursor;   /* next slot index to inspect */
    uint32_t kind;
    uint32_t _pad;
} ArrIter;

static void *arr_iter_create_with_kind(void *arr_p, uint32_t kind) {
    ArrIter *it = (ArrIter *)malloc(sizeof(ArrIter));
    it->header.refcount = 1;
    it->header.type_tag = __TORAJS_TAG_ARR_ITER;
    it->header.flags = 0;
    it->arr = arr_p;
    it->cursor = 0;
    it->kind = kind;
    it->_pad = 0;
    if (arr_p != NULL) __torajs_rc_inc(arr_p);
    return it;
}

void *__torajs_arr_iter_create_keys(void *arr_p) {
    return arr_iter_create_with_kind(arr_p, ARR_ITER_KEYS);
}

void *__torajs_arr_iter_create_values(void *arr_p) {
    return arr_iter_create_with_kind(arr_p, ARR_ITER_VALUES);
}

void *__torajs_arr_iter_create_entries(void *arr_p) {
    return arr_iter_create_with_kind(arr_p, ARR_ITER_ENTRIES);
}

/* Advance the iter + fill (out_tag, out_payload) per kind. Returns
 * 1 on hit, 0 when cursor has run past arr.length. */
int64_t __torajs_arr_iter_step(void *iter_p,
                               int64_t *out_tag, int64_t *out_payload) {
    if (!iter_p) {
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        return 0;
    }
    ArrIter *it = (ArrIter *)iter_p;
    void *arr = it->arr;
    if (!arr) {
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        return 0;
    }
    uint64_t len = *(uint64_t *)((uint8_t *)arr + 8);
    uint32_t i = (uint32_t)it->cursor;
    if ((uint64_t)i >= len) {
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        return 0;
    }
    uint8_t *slot_base = (uint8_t *)arr + __TORAJS_ARR_HDR_SIZE
                         + (size_t)i * __TORAJS_ANY_SLOT_BYTES;
    uint64_t slot_tag = *(uint64_t *)slot_base;
    uint64_t slot_val = *(uint64_t *)(slot_base + 8);
    it->cursor = (int64_t)(i + 1);
    switch (it->kind) {
    case ARR_ITER_KEYS:
        *out_tag = __TORAJS_ANY_I64;
        *out_payload = (int64_t)i;
        break;
    case ARR_ITER_VALUES:
        *out_tag = (int64_t)slot_tag;
        *out_payload = (int64_t)slot_val;
        break;
    case ARR_ITER_ENTRIES: {
        /* Yield `[index, value]` Array<Any>. Same alloc-and-pre-
         * dec-refcount pattern as Map.entries (map_iter_make_pair_arr). */
        void *out_arr = __torajs_arr_alloc_any(2);
        /* Element 0 — the i64 index (primitive, no rc_inc). */
        out_arr = __torajs_arr_push_any(out_arr,
                                        (uint64_t)__TORAJS_ANY_I64,
                                        (uint64_t)i);
        /* Element 1 — the slot's tagged value. Heap payload needs
         * rc_inc before push (push adopts the pre-bumped ref). */
        if ((slot_tag & 0xff) == (uint64_t)__TORAJS_ANY_HEAP && slot_val != 0) {
            __torajs_rc_inc((void *)(uintptr_t)slot_val);
        }
        out_arr = __torajs_arr_push_any(out_arr, slot_tag, slot_val);
        /* Caller's any_box will rc_inc out_arr (→ 2 with only one
         * drop chain → leak); pre-decrement so the inc lands at 1. */
        ((__torajs_heap_header_t *)out_arr)->refcount -= 1;
        *out_tag = __TORAJS_ANY_HEAP;
        *out_payload = (int64_t)(uintptr_t)out_arr;
        break;
    }
    default:
        *out_tag = __TORAJS_ANY_UNDEF;
        *out_payload = 0;
        break;
    }
    return 1;
}

/* rc-aware drop. Same shape as map_iter_drop. */
void __torajs_arr_iter_drop(void *p) {
    if (!p) return;
    ArrIter *it = (ArrIter *)p;
    if (it->header.flags & 4 /* STATIC_LITERAL */) return;
    it->header.refcount -= 1;
    if (it->header.refcount != 0) return;
    if (it->arr != NULL) __torajs_value_drop_heap(it->arr);
    free(it);
}
