#include <inttypes.h>
#include <regex.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    (void)argc;
    regex_t re;
    regcomp(&re, "x", REG_EXTENDED);
    regmatch_t m[1];

    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        const char *s = "x";
        if (regexec(&re, s, 1, m, 0) == 0) {
            // the match array's group 0 is a fresh string in js too
            char *m0 = strndup(s + m[0].rm_so, (size_t)(m[0].rm_eo - m[0].rm_so));
            total += 1;
            free(m0);
        }
    }
    regfree(&re);

    printf("%" PRId64 "\n", total);
    return 0;
}
