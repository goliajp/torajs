#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static uint64_t ackermann(uint64_t m, uint64_t n) {
    if (m == 0) {
        return n + 1;
    }
    if (n == 0) {
        return ackermann(m - 1, 1);
    }
    return ackermann(m - 1, ackermann(m, n - 1));
}

int main(void) {
    printf("%" PRIu64 "\n", ackermann(3, 8));
    return 0;
}
