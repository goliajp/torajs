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
static StrBuf replace_abc(const char *s, const char *with) {
    size_t n = strlen(s);
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
    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        const char *s = "abbc xxx abc yyy abbbbc";
        StrBuf r = replace_abc(s, "XY");
        total = total + (int64_t)r.len;
        free(r.ptr);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
