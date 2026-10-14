#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char key[8];
    bool has_hits;
    int64_t hits;
    int64_t n;
} Rec;

/* open addressing over FNV-1a, the shape tr's dictionary has */
#define CAP 256
typedef struct {
    const char *key;
    int64_t count;
} Slot;

static uint64_t fnv1a(const char *s) {
    uint64_t h = 1469598103934665603ULL;
    for (; *s; s++) {
        h ^= (uint8_t)*s;
        h *= 1099511628211ULL;
    }
    return h;
}

static Slot *lookup(Slot *table, const char *key) {
    uint64_t i = fnv1a(key) & (CAP - 1);
    for (;;) {
        if (table[i].key == NULL) {
            table[i].key = key;
            return &table[i];
        }
        if (strcmp(table[i].key, key) == 0) {
            return &table[i];
        }
        i = (i + 1) & (CAP - 1);
    }
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    Rec recs[64];
    for (int64_t i = 0; i < 64; i++) {
        snprintf(recs[i].key, sizeof recs[i].key, "k%" PRId64, i);
        recs[i].has_hits = i % 3 != 0;
        recs[i].hits = recs[i].has_hits ? i : 0;
        recs[i].n = i;
    }
    Slot *counts = calloc(CAP, sizeof(Slot));
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) {
        Rec *r = &recs[i & 63];
        lookup(counts, r->key)->count += 1;
        total = total + (r->has_hits ? r->hits : 1) + r->n;
    }
    int64_t sum = 0;
    int64_t keys = 0;
    for (int i = 0; i < CAP; i++) {
        if (counts[i].key != NULL) {
            sum += counts[i].count;
            keys++;
        }
    }
    printf("%" PRId64 " %" PRId64 " %" PRId64 "\n", total, sum, keys);
    free(counts);
    return 0;
}
