#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *ptr;
    size_t len;
} Str;

int main(int argc, char **argv) {
    (void)argc;
    /* mirrors main.rs: format!("item{}", j) -- each element is a heap string */
    int64_t pool = strtoll(argv[2], NULL, 10);
    Str *arr = malloc((size_t)pool * sizeof(Str));
    for (int64_t j = 0; j < pool; j++) {
        char buf[32];
        int len = snprintf(buf, sizeof buf, "item%" PRId64, j);
        arr[j].ptr = malloc((size_t)len);
        memcpy(arr[j].ptr, buf, (size_t)len);
        arr[j].len = (size_t)len;
    }

    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        const Str *s = &arr[i % pool];
        total = total + (int64_t)s->len;
    }
    printf("%" PRId64 "\n", total);

    for (int j = 0; j < pool; j++) {
        free(arr[j].ptr);
    }
    free(arr);
    return 0;
}
