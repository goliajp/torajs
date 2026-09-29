#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t ackermann(uint64_t m, uint64_t n) {
    if (m == 0) {
        return n + 1;
    }
    if (n == 0) {
        return ackermann(m - 1, 1);
    }
    return ackermann(m - 1, ackermann(m, n - 1));
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t m = strtoull(argv[1], NULL, 10);
    uint64_t n = strtoull(argv[2], NULL, 10);
    printf("%" PRIu64 "\n", ackermann(m, n));
    return 0;
}
