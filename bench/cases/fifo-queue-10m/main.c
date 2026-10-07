#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* heap-allocated growable ring buffer, mirrors std::collections::VecDeque */
typedef struct {
    int64_t *buf;
    size_t cap;
    size_t head;
    size_t len;
} Deque;

static void deque_init(Deque *q) {
    q->buf = NULL;
    q->cap = 0;
    q->head = 0;
    q->len = 0;
}

static void deque_grow(Deque *q) {
    size_t new_cap = q->cap == 0 ? 4 : q->cap * 2;
    int64_t *new_buf = malloc(new_cap * sizeof(int64_t));
    for (size_t i = 0; i < q->len; i++) {
        new_buf[i] = q->buf[(q->head + i) % q->cap];
    }
    free(q->buf);
    q->buf = new_buf;
    q->cap = new_cap;
    q->head = 0;
}

static void deque_push_back(Deque *q, int64_t v) {
    if (q->len == q->cap) {
        deque_grow(q);
    }
    q->buf[(q->head + q->len) % q->cap] = v;
    q->len++;
}

static int deque_pop_front(Deque *q, int64_t *out) {
    if (q->len == 0) {
        return 0;
    }
    *out = q->buf[q->head];
    q->head = (q->head + 1) % q->cap;
    q->len--;
    return 1;
}

int main(int argc, char **argv) {
    (void)argc;
    Deque q;
    deque_init(&q);
    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    size_t window = strtoull(argv[2], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        deque_push_back(&q, i);
        if (q.len > window) {
            int64_t v;
            deque_pop_front(&q, &v);
            total += v;
        }
    }
    int64_t v;
    while (deque_pop_front(&q, &v)) {
        total += v;
    }
    free(q.buf);
    printf("%" PRId64 "\n", total);
    return 0;
}
