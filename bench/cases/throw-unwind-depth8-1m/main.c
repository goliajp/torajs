#include <inttypes.h>
#include <setjmp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t code;
} Fail;

/* innermost active handler and the object in flight */
static sigjmp_buf *handler;
static Fail *thrown;

static int64_t finals = 0;

static void raise_fail(Fail *f) {
    thrown = f;
    siglongjmp(*handler, 1);
}

static int64_t descend(int64_t d, int64_t v, int64_t every) {
    if (d == 0) {
        if (v % every == 0) {
            Fail *f = malloc(sizeof(Fail));
            f->code = v;
            raise_fail(f);
        }
        return v;
    }
    sigjmp_buf env;
    sigjmp_buf *outer = handler;
    if (sigsetjmp(env, 0) != 0) {
        handler = outer;
        finals = finals + 1;
        raise_fail(thrown);
    }
    handler = &env;
    int64_t r = descend(d - 1, v + 1, every) + 1;
    handler = outer;
    finals = finals + 1;
    return r;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t n = strtoll(argv[1], NULL, 10);
    int64_t depth = strtoll(argv[2], NULL, 10);
    int64_t every = strtoll(argv[3], NULL, 10);

    /* total and i are not written between sigsetjmp and the jump back, so
       their values are defined in the catch arm */
    int64_t total = 0;
    sigjmp_buf env;
    for (int64_t i = 0; i < n; i++) {
        if (sigsetjmp(env, 0) == 0) {
            handler = &env;
            int64_t r = descend(depth, i, every);
            handler = NULL;
            total = total + r;
        } else {
            handler = NULL;
            total = total + thrown->code;
            free(thrown);
        }
    }
    printf("%" PRId64 " %" PRId64 "\n", total, finals);
    return 0;
}
