#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t *data;
    size_t len;
    size_t cap;
} I64Vec;

/* mirrors main.rs: xs.push(s) -- Vec's amortized-doubling growth */
static void vec_push(I64Vec *v, int64_t val) {
    if (v->len == v->cap) {
        size_t new_cap = v->cap == 0 ? 4 : v->cap * 2;
        v->data = realloc(v->data, new_cap * sizeof(int64_t));
        v->cap = new_cap;
    }
    v->data[v->len++] = val;
}

static I64Vec build(size_t n, int64_t seed) {
    I64Vec xs = {0};
    int32_t s = (int32_t)seed;
    for (size_t i = 0; i < n; i++) {
        /* the minimal standard generator; the product stays below 2^47 */
        s = (int32_t)(((int64_t)s * 48271) % 2147483647);
        if (s == 0) {
            s = 1;
        }
        vec_push(&xs, s);
    }
    return xs;
}

/* the (a, b) => a - b comparator, called through a function pointer by qsort */
static int cmp_i64(const void *pa, const void *pb) {
    int64_t d = *(const int64_t *)pa - *(const int64_t *)pb;
    return (d > 0) - (d < 0);
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t checksum = 0;
    int64_t passes = strtoll(argv[1], NULL, 10);
    size_t len = strtoull(argv[2], NULL, 10);
    for (int64_t p = 0; p < passes; p++) {
        I64Vec xs = build(len, p + 1);
        qsort(xs.data, xs.len, sizeof(int64_t), cmp_i64);
        checksum = checksum + xs.data[0] + xs.data[len - 1];
        free(xs.data);
    }
    printf("%" PRId64 "\n", checksum);
    return 0;
}
