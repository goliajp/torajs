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

/* stands in for the heap-allocated env of `move |x| x + offset` */
typedef struct {
    int64_t offset;
} ClosureEnv;

static int64_t add_offset(void *env, int64_t x) {
    return x + ((ClosureEnv *)env)->offset;
}

__attribute__((noinline))
static int64_t loop_sum(const int64_t *xs, size_t len, int64_t (*f)(void *, int64_t), void *env) {
    int64_t sum = 0;
    for (size_t i = 0; i < len; i++) {
        sum = sum + f(env, xs[i]);
    }
    return sum;
}

int main(void) {
    I64Vec xs = {0};
    for (int64_t i = 0; i < 10000000; i++) {
        vec_push(&xs, i);
    }

    int64_t offset = 2;
    // mirrors main.rs: black_box(2) opaques the literal
    __asm__ volatile("" : "+r"(offset) : : "memory");

    ClosureEnv env = { offset };
    // mirrors main.rs: Box<dyn Fn> + black_box(&*f) force a real indirect call
    int64_t (*volatile f_ref)(void *, int64_t) = add_offset;

    printf("%" PRId64 "\n", loop_sum(xs.data, xs.len, f_ref, &env));

    free(xs.data);
    return 0;
}
