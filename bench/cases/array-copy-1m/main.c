#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t *data;
    size_t len;
    size_t cap;
} I64Vec;

/* mirrors main.rs: src.push(i) / dst.push(...) — Vec's amortized-doubling growth */
static void vec_push(I64Vec *v, int64_t val) {
    if (v->len == v->cap) {
        size_t new_cap = v->cap == 0 ? 1 : v->cap * 2;
        v->data = realloc(v->data, new_cap * sizeof(int64_t));
        v->cap = new_cap;
    }
    v->data[v->len++] = val;
}

int main(void) {
    I64Vec src = {0};
    int64_t i = 0;
    while (i < 10000000) {
        vec_push(&src, i);
        i += 1;
    }

    I64Vec dst = {0};
    size_t j = 0;
    while (j < src.len) {
        int64_t val = src.data[j];
        // mirrors main.rs: black_box(src[j]) blocks the memcpy the loop is
        __asm__ volatile("" : "+r"(val) : : "memory");
        vec_push(&dst, val);
        j += 1;
    }

    printf("%" PRId64 "\n", (int64_t)dst.len + dst.data[9999999]);

    free(src.data);
    free(dst.data);
    return 0;
}
