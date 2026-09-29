#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    (void)argc;
    int64_t *xs = NULL;
    size_t len = 0;
    size_t cap = 0;
    int64_t n = strtoll(argv[1], NULL, 10);

    for (int64_t i = 0; i < n; i++) {
        if (len == cap) {
            /* mirrors Rust's Vec<i64> amortized-doubling growth on push */
            size_t new_cap = cap == 0 ? 1 : cap * 2;
            int64_t *new_xs = (int64_t *)realloc(xs, new_cap * sizeof(int64_t));
            xs = new_xs;
            cap = new_cap;
        }
        xs[len] = i;
        len++;
    }

    int64_t total = 0;
    while (len > 0) {
        len--;
        total += xs[len];
    }

    printf("%" PRId64 "\n", total);
    free(xs);
    return 0;
}
