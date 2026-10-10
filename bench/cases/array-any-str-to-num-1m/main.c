#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    int64_t pool = strtoll(argv[2], NULL, 10);
    /* each element is a heap string, as '' + j is in main.ts */
    char **arr = malloc((size_t)pool * sizeof(char *));
    for (int64_t j = 0; j < pool; j++) {
        char buf[32];
        int len = snprintf(buf, sizeof buf, "%" PRId64, j);
        arr[j] = malloc((size_t)len + 1);
        memcpy(arr[j], buf, (size_t)len + 1);
    }
    double total = 0.0;
    for (int64_t i = 0; i < n; i++) {
        total = total + strtod(arr[i % pool], NULL);
    }
    printf("%.17g\n", total);
    for (int64_t j = 0; j < pool; j++) {
        free(arr[j]);
    }
    free(arr);
    return 0;
}
