#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t x;
    int64_t y;
} Vec2;

/* heap-allocated, mirrors Box<Vec2> */
static Vec2 *vec2_new(int64_t x, int64_t y) {
    Vec2 *v = malloc(sizeof(Vec2));
    v->x = x;
    v->y = y;
    return v;
}

static int64_t vec2_norm(const Vec2 *v) {
    return v->x * v->x + v->y * v->y;
}

static Vec2 *vec2_scale(const Vec2 *v, int64_t k) {
    return vec2_new(v->x * k, v->y * k);
}

int main(void) {
    int64_t total = 0;
    int64_t n = 100000;
    for (int64_t i = 0; i < n; i++) {
        Vec2 *v = vec2_new(i, i + 1);
        // mirrors main.rs: black_box keeps both allocations
        __asm__ volatile("" : "+r"(v) : : "memory");
        Vec2 *s = vec2_scale(v, 2);
        __asm__ volatile("" : "+r"(s) : : "memory");
        total = total + vec2_norm(s);
        free(s);
        free(v);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
