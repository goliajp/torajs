#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t survivor(uint64_t people, uint64_t k) {
    uint64_t r = 0;
    for (uint64_t i = 2; i <= people; i++) {
        r = (r + k) % i;
    }
    return r;
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t limit = strtoull(argv[1], NULL, 10);
    uint64_t people = strtoull(argv[2], NULL, 10);
    uint64_t total = 0;
    for (uint64_t k = 1; k <= limit; k++) {
        total += survivor(people, k);
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
