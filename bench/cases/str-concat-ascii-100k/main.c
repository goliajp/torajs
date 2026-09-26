#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    const char *prefix = "  Hello42 word  ";
    size_t prefix_len = strlen(prefix);
    int64_t total = 0;
    int64_t n = 100000;
    for (int64_t i = 0; i < n; i++) {
        /* prefix + 'x' builds a fresh string of exactly the combined length */
        size_t len = prefix_len + 1;
        char *s = malloc(len);
        memcpy(s, prefix, prefix_len);
        s[prefix_len] = 'x';
        // mirrors main.rs: black_box keeps the allocation
        __asm__ volatile("" : "+r"(s) : : "memory");
        total = total + (int64_t)len;
        free(s);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
