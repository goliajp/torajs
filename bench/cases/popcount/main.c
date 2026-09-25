#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

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

int main(void) {
    uint64_t total = 0;
    for (uint64_t i = 0; i < 10000000; i++) {
        total += popcount(i);
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
