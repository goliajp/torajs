#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t smallest_factor(uint64_t n) {
    uint64_t d = 2;
    while (d * d <= n) {
        if (n % d == 0) {
            return d;
        }
        d += 1;
    }
    return n;
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t limit = strtoull(argv[1], NULL, 10);
    uint64_t total = 0;
    uint64_t n = 2;
    while (n < limit) {
        total += smallest_factor(n);
        n += 1;
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
