#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static uint64_t gcd(uint64_t a, uint64_t b) {
    while (b != 0) {
        uint64_t t = b;
        b = a % b;
        a = t;
    }
    return a;
}

int main(void) {
    uint64_t total = 0;
    uint64_t target = 1234567;
    for (uint64_t i = 1; i <= 1000000; i++) {
        total += gcd(i, target);
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
