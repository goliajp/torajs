#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* mirrors main.rs: line.split(',') — a lazy iterator, no array of parts */
static int64_t rebuild(const char *line) {
    int64_t total = 0;
    const char *p = line;
    while (*p != '\0') {
        const char *start = p;
        while (*p != '\0' && *p != ',') {
            p++;
        }
        size_t len = (size_t)(p - start);

        /* mirrors main.rs: format!("{}|", part) — a fresh heap allocation per part */
        char *s = malloc(len + 1);
        memcpy(s, start, len);
        s[len] = '|';
        total += (int64_t)(unsigned char)s[0];
        free(s);

        if (*p == ',') {
            p++;
        }
    }
    return total;
}

int main(void) {
    int64_t total = 0;
    int64_t n = 100000;
    for (int64_t i = 0; i < n; i++) {
        total += rebuild("alpha,beta,gamma,delta,epsilon,zeta");
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
