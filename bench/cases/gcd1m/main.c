#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t gcd(uint64_t a, uint64_t b) {
    while (b != 0) {
        uint64_t t = b;
        b = a % b;
        a = t;
    }
    return a;
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t total = 0;
    uint64_t limit = strtoull(argv[1], NULL, 10);
    uint64_t target = strtoull(argv[2], NULL, 10);
    for (uint64_t i = 1; i <= limit; i++) {
        total += gcd(i, target);
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
