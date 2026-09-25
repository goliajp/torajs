#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t *data;
    size_t len;
    size_t cap;
} I64Vec;

/* mirrors main.rs: xs.push(i) — Vec's amortized-doubling growth */
static void vec_push(I64Vec *v, int64_t val) {
    if (v->len == v->cap) {
        size_t new_cap = v->cap == 0 ? 1 : v->cap * 2;
        v->data = realloc(v->data, new_cap * sizeof(int64_t));
        v->cap = new_cap;
    }
    v->data[v->len++] = val;
}

static int64_t add1(int64_t x) {
    return x + 1;
}

static int64_t reduce(const int64_t *xs, size_t len, int64_t (*f)(int64_t)) {
    int64_t sum = 0;
    for (size_t i = 0; i < len; i++) {
        sum = sum + f(xs[i]);
    }
    return sum;
}

int main(void) {
    I64Vec xs = {0};
    for (int64_t i = 0; i < 10000000; i++) {
        vec_push(&xs, i);
    }

    // mirrors main.rs: black_box(add1 as fn(i64) -> i64) forces a real indirect call
    int64_t (*volatile f)(int64_t) = add1;

    printf("%" PRId64 "\n", reduce(xs.data, xs.len, f));

    free(xs.data);
    return 0;
}
