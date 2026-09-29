#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *ptr;
    size_t len;
    size_t cap;
} StrBuf;

typedef struct {
    const char *ptr;
    size_t len;
} Word;

#define WORD(s) {s, sizeof(s) - 1}

static const Word words[16] = {
    WORD("alpha"), WORD("beta"),   WORD("gamma"),   WORD("delta"),
    WORD("epsilon"), WORD("zeta"), WORD("eta"),     WORD("theta"),
    WORD("iota"),  WORD("kappa"),  WORD("lambda"),  WORD("mu"),
    WORD("nu"),    WORD("xi"),     WORD("omicron"), WORD("pi"),
};

static void append(StrBuf *b, const char *s, size_t len) {
    size_t need = b->len + len;
    if (need > b->cap) {
        size_t cap = b->cap ? b->cap : 8;
        while (cap < need) {
            cap *= 2;
        }
        b->ptr = realloc(b->ptr, cap);
        b->cap = cap;
    }
    memcpy(b->ptr + b->len, s, len);
    b->len = need;
}

/* the product stays below 2^53, so the low 31 bits are what | 0 then & keeps */
static int64_t next(int64_t s) {
    int64_t t = (int64_t)((uint64_t)(s * 48271) & 0x7fffffff);
    return t == 0 ? 1 : t;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t target = strtoll(argv[1], NULL, 10);
    int64_t rounds = strtoll(argv[2], NULL, 10);
    int64_t seed = strtoll(argv[3], NULL, 10);

    int64_t total = 0;
    for (int64_t r = 0; r < rounds; r++) {
        StrBuf s = {NULL, 0, 0};
        while ((int64_t)s.len < target) {
            seed = next(seed);
            const Word *w = &words[seed & 15];
            append(&s, w->ptr, w->len);
            append(&s, " ", 1);
        }
        total = total + (int64_t)s.len + (unsigned char)s.ptr[s.len >> 1];
        free(s.ptr);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
