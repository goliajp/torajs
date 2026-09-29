#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* heap-allocated growable array, mirrors Rust's Vec<i64> push growth */
typedef struct {
    int64_t *buf;
    size_t cap;
    size_t len;
} Vec;

static void vec_push(Vec *v, int64_t x) {
    if (v->len == v->cap) {
        size_t new_cap = v->cap == 0 ? 4 : v->cap * 2;
        v->buf = realloc(v->buf, new_cap * sizeof(int64_t));
        v->cap = new_cap;
    }
    v->buf[v->len++] = x;
}

static int64_t id_i64(int64_t x) {
    return x;
}

/* mirrors main.rs: #[inline(never)] on loop_sum */
__attribute__((noinline)) static int64_t loop_sum(const int64_t *xs, size_t len) {
    int64_t sum = 0;
    for (size_t i = 0; i < len; i++) {
        sum = sum + id_i64(xs[i]);
    }
    return sum;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t count = strtoll(argv[1], NULL, 10);
    Vec xs;
    xs.buf = NULL;
    xs.cap = 0;
    xs.len = 0;
    for (int64_t i = 0; i < count; i++) {
        vec_push(&xs, i);
    }
    printf("%" PRId64 "\n", loop_sum(xs.buf, xs.len));
    free(xs.buf);
    return 0;
}
