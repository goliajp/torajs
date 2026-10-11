#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    bool has_a;
    int64_t a;
    int64_t b;
} Opt;

static Opt make(int64_t i) {
    if (i % 4 == 0) {
        return (Opt){true, i, i};
    }
    return (Opt){false, 0, i};
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    int64_t total = 0;
    int64_t created = 0;
    for (int64_t i = 0; i < n; i++) {
        Opt o = make(i);
        if (!o.has_a) {
            o.has_a = true;
            o.a = 2;
            created++;
        }
        o.b = o.b + 1;
        total = total + (o.has_a ? o.a : 0) + o.b;
    }
    printf("%" PRId64 " %" PRId64 "\n", total, created);
    return 0;
}
