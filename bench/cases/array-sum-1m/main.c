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

int main(void) {
    I64Vec xs = {0};
    int64_t i = 0;
    while (i < 10000000) {
        vec_push(&xs, i);
        i += 1;
    }

    int64_t sum = 0;
    size_t j = 0;
    while (j < xs.len) {
        int64_t val = xs.data[j];
        // mirrors main.rs: black_box(xs[j]) blocks vectorization
        __asm__ volatile("" : "+r"(val) : : "memory");
        sum = sum + val;
        j += 1;
    }

    printf("%" PRId64 "\n", sum);

    free(xs.data);
    return 0;
}
