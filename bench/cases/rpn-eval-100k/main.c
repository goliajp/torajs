#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* mirrors main.rs: expr.split(' ') is a lazy iterator (no Vec collect), so
   this walks the string in place and never allocates a token array */
static int64_t eval_rpn(const char *expr) {
    int64_t stack[16] = {0};
    size_t sp = 0;

    const char *p = expr;
    for (;;) {
        const char *tok = p;
        while (*p != '\0' && *p != ' ') {
            p++;
        }

        int64_t c0 = (int64_t)(unsigned char)tok[0];
        if (c0 >= 48 && c0 <= 57) {
            stack[sp] = c0 - 48;
            sp += 1;
        } else {
            int64_t b = stack[sp - 1];
            int64_t a = stack[sp - 2];
            sp -= 2;
            int64_t r;
            if (c0 == 43) {
                r = a + b;
            } else if (c0 == 45) {
                r = a - b;
            } else {
                r = a * b;
            }
            stack[sp] = r;
            sp += 1;
        }

        if (*p == '\0') {
            break;
        }
        p++; /* skip the separator */
    }

    return stack[0];
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        total += eval_rpn("3 4 + 2 * 5 +");
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
