#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t fst;
    int64_t snd;
} Pair;

static int64_t loop_sum(int64_t n) {
    int64_t sum = 0;
    for (int64_t i = 0; i < n; i++) {
        Pair p = {i, i + 1};
        sum = sum + p.fst + p.snd;
    }
    return sum;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    printf("%" PRId64 "\n", loop_sum(n));
    return 0;
}
