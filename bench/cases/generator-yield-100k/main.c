#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

enum { STATE_START, STATE_AFTER_YIELD, STATE_DONE };

/* function* range(start, end) as its generator state machine; the object
   lives on the heap like a js generator object */
typedef struct {
    int state;
    int64_t start;
    int64_t end;
    int64_t i;
} Range;

static Range *range(int64_t start, int64_t end) {
    Range *g = malloc(sizeof *g);
    *g = (Range){STATE_START, start, end, 0};
    return g;
}

/* one resumption; false is the { done: true } result */
static bool range_next(Range *g, int64_t *out) {
    switch (g->state) {
    case STATE_START:
        g->i = g->start;
        break;
    case STATE_AFTER_YIELD:
        g->i = g->i + 1;
        break;
    default:
        return false;
    }
    if (g->i < g->end) {
        g->state = STATE_AFTER_YIELD;
        *out = g->i;
        return true;
    }
    g->state = STATE_DONE;
    return false;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t total = 0;
    int passes = (int)strtoll(argv[1], NULL, 10);
    int64_t end = strtoll(argv[2], NULL, 10);
    for (int p = 0; p < passes; p++) {
        Range *g = range(0, end);
        int64_t v;
        while (range_next(g, &v)) {
            total = total + v;
        }
        free(g);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
