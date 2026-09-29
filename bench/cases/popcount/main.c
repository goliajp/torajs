#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* mirrors main.rs: hand-rolled Kernighan bit-trick, not a hardware popcount intrinsic */
static uint64_t popcount(uint64_t x) {
    uint64_t n = x;
    uint64_t count = 0;
    while (n != 0) {
        n &= n - 1;
        count += 1;
    }
    return count;
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t limit = strtoull(argv[1], NULL, 10);
    uint64_t total = 0;
    for (uint64_t i = 0; i < limit; i++) {
        total += popcount(i);
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
