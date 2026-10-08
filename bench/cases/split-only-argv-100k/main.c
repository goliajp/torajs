#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    const char *ptr;
    size_t len;
} str_slice;

/* mirrors main.rs: input.split(sep).collect() materializes a Vec<&str>,
   so this grows a slice array with the same amortized-doubling push loop */
static size_t split_on(const char *s, const char *sep, size_t sep_len, str_slice **out) {
    str_slice *v = NULL;
    size_t len = 0;
    size_t cap = 0;

    const char *p = s;
    for (;;) {
        const char *hit = strstr(p, sep);
        const char *end = hit ? hit : p + strlen(p);
        if (len == cap) {
            cap = cap == 0 ? 1 : cap * 2;
            v = (str_slice *)realloc(v, cap * sizeof(str_slice));
        }
        v[len].ptr = p;
        v[len].len = (size_t)(end - p);
        len++;

        if (!hit) {
            break;
        }
        p = hit + sep_len;
    }

    *out = v;
    return len;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    const char *input = argv[2];
    const char *sep = argv[3];
    size_t sep_len = strlen(sep);

    for (int64_t i = 0; i < n; i++) {
        str_slice *parts = NULL;
        size_t count = split_on(input, sep, sep_len, &parts);
        total += (int64_t)count;
        free(parts);
    }

    printf("%" PRId64 "\n", total);
    return 0;
}
