#include <inttypes.h>
#include <regex.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    regex_t re;
    // without REG_NEWLINE an ERE `.` already matches `\n`, which is what the s flag asks for
    regcomp(&re, "a.+c", REG_EXTENDED);
    regmatch_t m[1];

    int64_t total = 0;
    int64_t n = 100000;
    for (int64_t i = 0; i < n; i++) {
        char *s = (char *)malloc(64);
        snprintf(s, 64, "pre a\nmiddle\nc post %" PRId64, i);
        if (regexec(&re, s, 1, m, 0) == 0) {
            char *m0 = strndup(s + m[0].rm_so, (size_t)(m[0].rm_eo - m[0].rm_so));
            total += (int64_t)strlen(m0);
            free(m0);
        }
        free(s);
    }
    regfree(&re);

    printf("%" PRId64 "\n", total);
    return 0;
}
