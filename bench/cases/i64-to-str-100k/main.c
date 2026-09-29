#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *ptr;
    size_t len;
} Str;

/* mirrors main.rs: i.to_string() -- digits formatted on the stack, then
   copied into a fresh heap string of exactly that length */
static Str i64_to_str(int64_t v) {
    char buf[20];
    size_t pos = sizeof buf;
    uint64_t u = v < 0 ? 0 - (uint64_t)v : (uint64_t)v;
    do {
        buf[--pos] = (char)('0' + u % 10);
        u /= 10;
    } while (u != 0);
    if (v < 0) {
        buf[--pos] = '-';
    }
    Str s;
    s.len = sizeof buf - pos;
    s.ptr = malloc(s.len);
    memcpy(s.ptr, buf + pos, s.len);
    return s;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        Str s = i64_to_str(i);
        // main.rs's String escapes through the formatter; without this clang
        // elides the malloc / free pair
        __asm__ volatile("" : "+r"(s.ptr) : : "memory");
        total = total + (int64_t)s.len;
        free(s.ptr);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
