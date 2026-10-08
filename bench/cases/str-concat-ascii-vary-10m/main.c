#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    (void)argc;
    const char *prefix = "  Hello42 word  ";
    const char *tails[2] = {argv[2], argv[3]};
    size_t tail_lens[2] = {strlen(argv[2]), strlen(argv[3])};
    size_t prefix_len = strlen(prefix);
    int64_t n = strtoll(argv[1], NULL, 10);
    char *ring[8] = {0};
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) {
        /* prefix + tail builds a fresh string of exactly the combined length */
        const char *tail = tails[i & 1];
        size_t tail_len = tail_lens[i & 1];
        size_t len = prefix_len + tail_len;
        char *s = malloc(len + 1);
        memcpy(s, prefix, prefix_len);
        memcpy(s + prefix_len, tail, tail_len);
        s[len] = 0;
        free(ring[i & 7]);
        ring[i & 7] = s;
        total = total + (unsigned char)s[i & 15];
    }
    printf("%" PRId64 "\n%s\n", total, ring[(n - 1) & 7]);
    return 0;
}
