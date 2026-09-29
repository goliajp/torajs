#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int is_prime(uint64_t n) {
    if (n < 2) {
        return 0;
    }
    uint64_t i = 2;
    while (i * i <= n) {
        if (n % i == 0) {
            return 0;
        }
        i += 1;
    }
    return 1;
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t limit = strtoull(argv[1], NULL, 10);
    uint64_t count = 0;
    uint64_t n = 0;
    while (n < limit) {
        if (is_prime(n)) {
            count += 1;
        }
        n += 1;
    }
    printf("%" PRIu64 "\n", count);
    return 0;
}
