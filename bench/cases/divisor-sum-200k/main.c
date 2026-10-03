#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t divisor_sum(uint64_t n) {
    uint64_t s = 0;
    uint64_t d = 1;
    while (d * d <= n) {
        if (n % d == 0) {
            s += d;
        }
        d += 1;
    }
    return s;
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t limit = strtoull(argv[1], NULL, 10);
    uint64_t total = 0;
    for (uint64_t n = 1; n <= limit; n++) {
        total += divisor_sum(n);
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
