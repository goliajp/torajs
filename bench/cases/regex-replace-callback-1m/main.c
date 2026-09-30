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

/* an immutable string value: one allocation holding the length and the bytes */
typedef struct {
    size_t len;
    char data[];
} Str;

typedef struct {
    size_t index;
    size_t key_end;
    size_t end;
} Match;

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

/* the minimal standard generator; the product stays below 2^47 */
static int64_t next(int64_t s) {
    int64_t t = (s * 48271) % 2147483647;
    return t == 0 ? 1 : t;
}

static Str *str_new(const char *s, size_t len) {
    Str *p = malloc(sizeof(Str) + len);
    p->len = len;
    memcpy(p->data, s, len);
    return p;
}

static int is_lower(char c) {
    return c >= 'a' && c <= 'z';
}

static int is_digit(char c) {
    return c >= '0' && c <= '9';
}

/* ([a-z]+)=(\d+): every start inside one run of letters reaches the same end,
   so a run that fails is skipped whole */
static int find(const char *s, size_t len, size_t from, Match *m) {
    size_t p = from;
    while (p < len) {
        if (!is_lower(s[p])) {
            p++;
            continue;
        }
        size_t q = p + 1;
        while (q < len && is_lower(s[q])) {
            q++;
        }
        if (q + 1 < len && s[q] == '=' && is_digit(s[q + 1])) {
            size_t r = q + 2;
            while (r < len && is_digit(s[r])) {
                r++;
            }
            m->index = p;
            m->key_end = q;
            m->end = r;
            return 1;
        }
        p = q;
    }
    return 0;
}

static Str *replacer(const Str *k, const Str *v) {
    Str *r = malloc(sizeof(Str) + v->len + 1 + k->len);
    r->len = v->len + 1 + k->len;
    memcpy(r->data, v->data, v->len);
    r->data[v->len] = ':';
    memcpy(r->data + v->len + 1, k->data, k->len);
    return r;
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
            char key[8];
            char value[6];
            seed = next(seed);
            int64_t shape = seed % 8;
            seed = next(seed);
            size_t klen = (size_t)(1 + seed % 8);
            for (size_t k = 0; k < klen; k++) {
                seed = next(seed);
                key[k] = (char)('a' + seed % 26);
            }
            seed = next(seed);
            size_t vlen = (size_t)(1 + seed % 6);
            for (size_t k = 0; k < vlen; k++) {
                seed = next(seed);
                value[k] = (char)('0' + seed % 10);
            }
            if (shape != 1) {
                append(&line, key, klen);
            }
            if (shape != 0) {
                append(&line, "=", 1);
            }
            append(&line, value, vlen);
            append(&line, ";", 1);
        }
        lines[i] = line;
    }

    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) {
        const StrBuf *line = &lines[i & 1023];
        StrBuf out = {NULL, 0, 0};
        size_t last = 0;
        Match m;
        while (find(line->ptr, line->len, last, &m)) {
            Str *key = str_new(line->ptr + m.index, m.key_end - m.index);
            Str *value = str_new(line->ptr + m.key_end + 1, m.end - m.key_end - 1);
            Str *rep = replacer(key, value);
            append(&out, line->ptr + last, m.index - last);
            append(&out, rep->data, rep->len);
            free(rep);
            free(key);
            free(value);
            last = m.end;
        }
        append(&out, line->ptr + last, line->len - last);
        total = total + (int64_t)out.len + (unsigned char)out.ptr[out.len >> 1];
        free(out.ptr);
    }
    printf("%" PRId64 "\n", total);

    for (int i = 0; i < 1024; i++) {
        free(lines[i].ptr);
    }
    return 0;
}
