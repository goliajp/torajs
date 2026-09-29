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

/* stands in for the heap-allocated env of `move |x| x + k` */
typedef struct {
    int64_t k;
} ClosureEnv;

static int64_t add_k(void *env, int64_t x) {
    return x + ((ClosureEnv *)env)->k;
}

__attribute__((noinline))
static int64_t loop_sum(int64_t n, int64_t k) {
    I64Vec xs = {0};
    for (int64_t i = 0; i < n; i++) {
        vec_push(&xs, i);
    }

    ClosureEnv env = { k };
    // mirrors main.rs: Box<dyn Fn> + black_box(&*f) force a real indirect call
    int64_t (*volatile f_ref)(void *, int64_t) = add_k;

    I64Vec ys = {0};
    for (size_t i = 0; i < xs.len; i++) {
        vec_push(&ys, f_ref(&env, xs.data[i]));
    }

    int64_t sum = 0;
    for (size_t i = 0; i < ys.len; i++) {
        sum = sum + ys.data[i];
    }

    free(xs.data);
    free(ys.data);
    return sum;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    int64_t k = strtoll(argv[2], NULL, 10);
    printf("%" PRId64 "\n", loop_sum(n, k));
    return 0;
}
