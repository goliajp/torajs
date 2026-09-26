#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *ptr;
    size_t len;
} Str;

int main(void) {
    /* mirrors main.rs: format!("item{}", j) -- each element is a heap string */
    Str arr[100];
    for (int64_t j = 0; j < 100; j++) {
        char buf[32];
        int len = snprintf(buf, sizeof buf, "item%" PRId64, j);
        arr[j].ptr = malloc((size_t)len);
        memcpy(arr[j].ptr, buf, (size_t)len);
        arr[j].len = (size_t)len;
    }

    int64_t total = 0;
    int64_t n = 100000;
    for (int64_t i = 0; i < n; i++) {
        const Str *s = &arr[i % 100];
        total = total + (int64_t)s->len;
    }
    printf("%" PRId64 "\n", total);

    for (int j = 0; j < 100; j++) {
        free(arr[j].ptr);
    }
    return 0;
}
