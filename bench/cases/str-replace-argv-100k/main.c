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

/* mirrors main.rs: String growth -- max(2 * cap, needed, 8) */
static void buf_push_str(StrBuf *b, const char *s, size_t len) {
    size_t need = b->len + len;
    if (need > b->cap) {
        size_t cap = b->cap * 2;
        if (cap < need) {
            cap = need;
        }
        if (cap < 8) {
            cap = 8;
        }
        b->ptr = realloc(b->ptr, cap);
        b->cap = cap;
    }
    memcpy(b->ptr + b->len, s, len);
    b->len += len;
}

/* hand-coded matcher for /a(b+)c/g: b+ is greedy and backtracking can only
   hand a 'b' to c, which never matches, so one forward scan is exact */
static StrBuf replace_abc(const char *s, size_t n, const char *with) {
    size_t with_len = strlen(with);
    StrBuf out = {0};
    size_t i = 0;
    size_t last = 0;
    while (i < n) {
        if (s[i] == 'a') {
            size_t j = i + 1;
            while (j < n && s[j] == 'b') {
                j++;
            }
            if (j > i + 1 && j < n && s[j] == 'c') {
                buf_push_str(&out, s + last, i - last);
                buf_push_str(&out, with, with_len);
                i = j + 1;
                last = i;
                continue;
            }
        }
        i++;
    }
    buf_push_str(&out, s + last, n - last);
    return out;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    /* the comma-separated words of argv[2] */
    const char *words[64];
    size_t word_len[64];
    size_t nw = 0;
    const char *p = argv[2];
    for (;;) {
        const char *q = strchr(p, ',');
        size_t l = q ? (size_t)(q - p) : strlen(p);
        words[nw] = p;
        word_len[nw] = l;
        nw++;
        if (!q) {
            break;
        }
        p = q + 1;
    }
    StrBuf subjects[16];
    for (size_t k = 0; k < 16; k++) {
        StrBuf s = {0};
        for (size_t w = 0; w < nw; w++) {
            if (w > 0) {
                buf_push_str(&s, " ", 1);
            }
            size_t at = (w + k) % nw;
            buf_push_str(&s, words[at], word_len[at]);
        }
        subjects[k] = s;
    }
    int64_t total = 0;
    StrBuf last = {0};
    for (int64_t i = 0; i < n; i++) {
        StrBuf *s = &subjects[i & 15];
        StrBuf r = replace_abc(s->ptr, s->len, "XY");
        total = total + (int64_t)r.len + (unsigned char)r.ptr[i % (int64_t)r.len];
        free(last.ptr);
        last = r;
    }
    printf("%.*s\n", (int)last.len, last.ptr);
    printf("%" PRId64 "\n", total);
    return 0;
}
