#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

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

int main(void) {
    uint64_t count = 0;
    uint64_t n = 0;
    while (n < 1000000) {
        if (is_prime(n)) {
            count += 1;
        }
        n += 1;
    }
    printf("%" PRIu64 "\n", count);
    return 0;
}
