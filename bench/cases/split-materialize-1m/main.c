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

/* a field of the line: a view into its bytes, not a copy */
typedef struct {
    const char *ptr;
    size_t len;
} Slice;

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

static void append_int(StrBuf *b, int64_t v) {
    char digits[24];
    int len = snprintf(digits, sizeof digits, "%" PRId64, v);
    append(b, digits, (size_t)len);
}

/* the product stays below 2^53, so the low 31 bits are what | 0 then & keeps */
static int64_t next(int64_t s) {
    int64_t t = (int64_t)((uint64_t)(s * 48271) & 0x7fffffff);
    return t == 0 ? 1 : t;
}

static Slice *split(const char *s, size_t len, char sep, size_t *count) {
    size_t n = 1;
    for (size_t i = 0; i < len; i++) {
        n += s[i] == sep;
    }
    Slice *parts = malloc(n * sizeof(Slice));
    size_t k = 0;
    size_t start = 0;
    for (size_t i = 0; i < len; i++) {
        if (s[i] == sep) {
            parts[k].ptr = s + start;
            parts[k].len = i - start;
            k++;
            start = i + 1;
        }
    }
    parts[k].ptr = s + start;
    parts[k].len = len - start;
    *count = n;
    return parts;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    int64_t fields = strtoll(argv[2], NULL, 10);
    int64_t seed = strtoll(argv[3], NULL, 10);

    StrBuf lines[1024];
    for (int i = 0; i < 1024; i++) {
        StrBuf line = {NULL, 0, 0};
        for (int64_t f = 0; f < fields; f++) {
            seed = next(seed);
            if (f > 0) {
                append(&line, ",", 1);
            }
            append_int(&line, seed % 100000);
        }
        lines[i] = line;
    }

    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) {
        const StrBuf *line = &lines[i & 1023];
        size_t count;
        Slice *parts = split(line->ptr, line->len, ',', &count);
        StrBuf rebuilt = {NULL, 0, 0};
        for (size_t k = 0; k < count; k++) {
            const Slice *p = &parts[k];
            total = total + (unsigned char)p->ptr[p->len - 1];
            append(&rebuilt, p->ptr, p->len);
            append(&rebuilt, ";", 1);
        }
        total = total + (int64_t)rebuilt.len;
        free(rebuilt.ptr);
        free(parts);
    }
    printf("%" PRId64 "\n", total);

    for (int i = 0; i < 1024; i++) {
        free(lines[i].ptr);
    }
    return 0;
}
