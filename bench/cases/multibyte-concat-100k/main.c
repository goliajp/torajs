#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* js strings are UTF-16 and .length counts code units, so the string is a
   uint16_t buffer here, as main.rs uses Vec<u16> */
typedef struct {
    uint16_t *ptr;
    size_t len;
} U16Str;

/* a + b builds a fresh string of exactly the combined length */
static U16Str concat(U16Str a, U16Str b) {
    U16Str out;
    out.len = a.len + b.len;
    out.ptr = malloc(out.len * sizeof(uint16_t));
    if (a.len != 0) {
        memcpy(out.ptr, a.ptr, a.len * sizeof(uint16_t));
    }
    memcpy(out.ptr + a.len, b.ptr, b.len * sizeof(uint16_t));
    return out;
}

int main(void) {
    int64_t total = 0;
    static uint16_t piece_units[] = u"中文测试";
    U16Str piece = {piece_units, sizeof piece_units / sizeof(uint16_t) - 1};
    int64_t n = 1000;
    for (int64_t i = 0; i < n; i++) {
        U16Str acc = {NULL, 0};
        for (int j = 0; j < 100; j++) {
            U16Str next = concat(acc, piece);
            free(acc.ptr);
            acc = next;
        }
        total = total + (int64_t)acc.len;
        free(acc.ptr);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
