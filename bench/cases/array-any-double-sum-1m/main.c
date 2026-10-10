#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    int64_t pool = strtoll(argv[2], NULL, 10);
    double *arr = malloc((size_t)pool * sizeof(double));
    for (int64_t j = 0; j < pool; j++) {
        arr[j] = (double)j + 0.5;
    }
    double total = 0.0;
    for (int64_t i = 0; i < n; i++) {
        total = total + arr[i % pool];
    }
    printf("%.17g\n", total);
    free(arr);
    return 0;
}
