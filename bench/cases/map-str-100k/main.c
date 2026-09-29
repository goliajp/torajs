#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint64_t rotl64(uint64_t x, int b) {
    return (x << b) | (x >> (64 - b));
}

static void sipround(uint64_t *v0, uint64_t *v1, uint64_t *v2, uint64_t *v3) {
    *v0 += *v1;
    *v1 = rotl64(*v1, 13);
    *v1 ^= *v0;
    *v0 = rotl64(*v0, 32);
    *v2 += *v3;
    *v3 = rotl64(*v3, 16);
    *v3 ^= *v2;
    *v0 += *v3;
    *v3 = rotl64(*v3, 21);
    *v3 ^= *v0;
    *v2 += *v1;
    *v1 = rotl64(*v1, 17);
    *v1 ^= *v2;
    *v2 = rotl64(*v2, 32);
}

/* mirrors main.rs: std::collections::HashMap's default hasher is
   SipHash-1-3 (1 compression round per 8-byte block, 3 finalization
   rounds) -- Rust switched from the original SipHash-2-4 for lower
   per-key hashing cost while keeping the same DoS-resistance shape.
   Implemented here so the hashing cost of the C port is comparable.
   The two 64-bit keys below are fixed constants, not process-randomized
   like Rust's RandomState: this bench only needs stable, comparable
   timing across runs, not DoS resistance. */
static uint64_t siphash13(const uint8_t *data, size_t len, uint64_t k0, uint64_t k1) {
    uint64_t v0 = 0x736f6d6570736575ULL ^ k0;
    uint64_t v1 = 0x646f72616e646f6dULL ^ k1;
    uint64_t v2 = 0x6c7967656e657261ULL ^ k0;
    uint64_t v3 = 0x7465646279746573ULL ^ k1;

    size_t left = len & 7;
    const uint8_t *end = data + (len - left);

    for (size_t i = 0; data + i < end; i += 8) {
        uint64_t m;
        memcpy(&m, data + i, 8);
        v3 ^= m;
        sipround(&v0, &v1, &v2, &v3); /* c-rounds = 1 */
        v0 ^= m;
    }

    uint64_t b = (uint64_t)len << 56;
    if (left >= 7) b |= (uint64_t)end[6] << 48;
    if (left >= 6) b |= (uint64_t)end[5] << 40;
    if (left >= 5) b |= (uint64_t)end[4] << 32;
    if (left >= 4) b |= (uint64_t)end[3] << 24;
    if (left >= 3) b |= (uint64_t)end[2] << 16;
    if (left >= 2) b |= (uint64_t)end[1] << 8;
    if (left >= 1) b |= (uint64_t)end[0];

    v3 ^= b;
    sipround(&v0, &v1, &v2, &v3);
    v0 ^= b;

    v2 ^= 0xff;
    sipround(&v0, &v1, &v2, &v3);
    sipround(&v0, &v1, &v2, &v3);
    sipround(&v0, &v1, &v2, &v3); /* d-rounds = 3 */

    return (v0 ^ v1) ^ (v2 ^ v3);
}

#define HASH_K0 0x0706050403020100ULL
#define HASH_K1 0x0f0e0d0c0b0a0908ULL

/* small open-addressing hash table (linear probing), standing in for
   std::collections::HashMap<String, i64> */
typedef struct {
    char *key; /* owned copy, malloc'd like Rust's String */
    size_t key_len;
    int64_t value;
    int occupied;
} hm_entry;

typedef struct {
    hm_entry *entries;
    size_t cap; /* power of two */
    size_t len;
} hash_map;

static void hm_init(hash_map *m) {
    m->entries = NULL;
    m->cap = 0;
    m->len = 0;
}

static size_t hm_find_slot(hm_entry *entries, size_t cap, const char *key, size_t key_len, uint64_t hash) {
    size_t mask = cap - 1;
    size_t idx = (size_t)hash & mask;
    for (;;) {
        if (!entries[idx].occupied) {
            return idx;
        }
        if (entries[idx].key_len == key_len && memcmp(entries[idx].key, key, key_len) == 0) {
            return idx;
        }
        idx = (idx + 1) & mask;
    }
}

static void hm_grow(hash_map *m) {
    size_t new_cap = m->cap == 0 ? 16 : m->cap * 2;
    hm_entry *new_entries = (hm_entry *)calloc(new_cap, sizeof(hm_entry));

    for (size_t i = 0; i < m->cap; i++) {
        if (m->entries[i].occupied) {
            uint64_t h = siphash13((const uint8_t *)m->entries[i].key, m->entries[i].key_len, HASH_K0, HASH_K1);
            size_t slot = hm_find_slot(new_entries, new_cap, m->entries[i].key, m->entries[i].key_len, h);
            new_entries[slot] = m->entries[i];
        }
    }

    free(m->entries);
    m->entries = new_entries;
    m->cap = new_cap;
}

/* mirrors main.rs: *m.entry(key).or_insert(0) += i -- key is always freshly
   allocated by format!() before entry() is called; if the key already
   exists, the freshly-allocated key is dropped (freed) instead of stored */
static void hm_add(hash_map *m, char *key, size_t key_len, int64_t delta) {
    if (m->cap == 0 || m->len * 4 >= m->cap * 3) {
        hm_grow(m);
    }

    uint64_t h = siphash13((const uint8_t *)key, key_len, HASH_K0, HASH_K1);
    size_t slot = hm_find_slot(m->entries, m->cap, key, key_len, h);

    if (m->entries[slot].occupied) {
        m->entries[slot].value += delta;
        free(key);
    } else {
        m->entries[slot].key = key;
        m->entries[slot].key_len = key_len;
        m->entries[slot].value = delta;
        m->entries[slot].occupied = 1;
        m->len++;
    }
}

/* mirrors main.rs: format!("key{}", j) / format!("ключ{}", j) -- a fresh
   heap allocation per call, same as a Rust String */
static char *make_key(int64_t i, int64_t j, size_t *out_len) {
    char buf[64];
    int len;
    if (i % 2 == 0) {
        len = snprintf(buf, sizeof buf, "key%" PRId64, j);
    } else {
        len = snprintf(buf, sizeof buf, "ключ%" PRId64, j);
    }
    char *key = (char *)malloc((size_t)len + 1);
    memcpy(key, buf, (size_t)len + 1);
    *out_len = (size_t)len;
    return key;
}

int main(int argc, char **argv) {
    (void)argc;
    hash_map m;
    hm_init(&m);

    int64_t n = strtoll(argv[1], NULL, 10);
    int64_t modulus = strtoll(argv[2], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        int64_t j = i % modulus;
        size_t key_len;
        char *key = make_key(i, j, &key_len);
        hm_add(&m, key, key_len, i);
    }

    int64_t total = 0;
    for (size_t idx = 0; idx < m.cap; idx++) {
        if (m.entries[idx].occupied) {
            total += m.entries[idx].value;
        }
    }

    printf("%zu %" PRId64 "\n", m.len, total);

    for (size_t idx = 0; idx < m.cap; idx++) {
        if (m.entries[idx].occupied) {
            free(m.entries[idx].key);
        }
    }
    free(m.entries);
    return 0;
}
