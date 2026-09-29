#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t fib(uint64_t n) {
    if (n < 2) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

int main(int argc, char **argv) {
    (void)argc;
    uint64_t n = strtoull(argv[1], NULL, 10);
    printf("%" PRIu64 "\n", fib(n));
    return 0;
}
