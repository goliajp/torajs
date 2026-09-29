#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    const char *ptr;
    size_t len;
} str_slice;

/* mirrors main.rs: "...".split(' ').collect() materializes a Vec<&str>,
   so this grows a slice array with the same amortized-doubling push loop */
static size_t split_space(const char *s, str_slice **out) {
    str_slice *v = NULL;
    size_t len = 0;
    size_t cap = 0;

    const char *p = s;
    for (;;) {
        const char *start = p;
        while (*p != '\0' && *p != ' ') {
            p++;
        }
        if (len == cap) {
            cap = cap == 0 ? 1 : cap * 2;
            v = (str_slice *)realloc(v, cap * sizeof(str_slice));
        }
        v[len].ptr = start;
        v[len].len = (size_t)(p - start);
        len++;

        if (*p == '\0') {
            break;
        }
        p++;
    }

    *out = v;
    return len;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    const char *s = "3 4 + 2 * 5 +";

    for (int64_t i = 0; i < n; i++) {
        str_slice *parts = NULL;
        size_t count = split_space(s, &parts);
        total += (int64_t)count;
        free(parts);
    }

    printf("%" PRId64 "\n", total);
    return 0;
}
