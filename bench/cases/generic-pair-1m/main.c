#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t fst;
    int64_t snd;
} Pair;

/* mirrors main.rs: #[inline(never)] on loop_sum */
__attribute__((noinline)) static int64_t loop_sum(int64_t n) {
    int64_t sum = 0;
    for (int64_t i = 0; i < n; i++) {
        /* heap-allocated, mirrors Box<Pair<i64, i64>> */
        Pair *p = malloc(sizeof(Pair));
        p->fst = i;
        p->snd = i + 1;
        sum = sum + p->fst + p->snd;
        free(p);
    }
    return sum;
}

int main(void) {
    printf("%" PRId64 "\n", loop_sum(1000000));
    return 0;
}
