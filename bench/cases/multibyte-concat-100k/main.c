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
    size_t cap;
} U16Str;

/* acc = acc + piece on a uniquely owned acc appends in place, capacity doubling */
static void append(U16Str *a, const uint16_t *b, size_t b_len) {
    size_t need = a->len + b_len;
    if (need > a->cap) {
        size_t cap = a->cap ? a->cap : 8;
        while (cap < need) {
            cap *= 2;
        }
        a->ptr = realloc(a->ptr, cap * sizeof(uint16_t));
        a->cap = cap;
    }
    memcpy(a->ptr + a->len, b, b_len * sizeof(uint16_t));
    a->len = need;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t total = 0;
    static uint16_t piece_units[] = u"中文测试";
    U16Str piece = {piece_units, sizeof piece_units / sizeof(uint16_t) - 1, 0};
    int64_t n = strtoll(argv[1], NULL, 10);
    int pieces = (int)strtoll(argv[2], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        U16Str acc = {NULL, 0, 0};
        for (int j = 0; j < pieces; j++) {
            append(&acc, piece.ptr, piece.len);
        }
        total = total + (int64_t)acc.len;
        free(acc.ptr);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
