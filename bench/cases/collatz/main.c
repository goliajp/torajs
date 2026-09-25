#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static uint64_t steps(uint64_t n) {
    uint64_t count = 0;
    while (n != 1) {
        if ((n & 1) == 0) {
            n >>= 1;
        } else {
            n = 3 * n + 1;
        }
        count += 1;
    }
    return count;
}

int main(void) {
    uint64_t max = 0;
    uint64_t i = 1;
    while (i <= 1000000) {
        uint64_t s = steps(i);
        if (s > max) {
            max = s;
        }
        i += 1;
    }
    printf("%" PRIu64 "\n", max);
    return 0;
}
