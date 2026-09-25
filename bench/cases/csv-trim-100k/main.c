#include <ctype.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int64_t row_len(const char *line) {
    int64_t total = 0;
    const char *p = line;
    for (;;) {
        const char *comma = strchr(p, ',');
        size_t len = comma ? (size_t)(comma - p) : strlen(p);
        const char *start = p;
        const char *end = p + len;
        while (start < end && isspace((unsigned char)*start)) {
            start++;
        }
        while (end > start && isspace((unsigned char)*(end - 1))) {
            end--;
        }
        total += (int64_t)(end - start);
        if (!comma) {
            break;
        }
        p = comma + 1;
    }
    return total;
}

int main(void) {
    int64_t total = 0;
    int64_t n = 100000;
    for (int64_t i = 0; i < n; i++) {
        total += row_len("  alpha , beta , gamma , delta , epsilon , zeta  ");
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
