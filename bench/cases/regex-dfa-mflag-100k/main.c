#include <inttypes.h>
#include <regex.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    regex_t re;
    // REG_NEWLINE lets `^` follow `\n`; the alternation adds the other js line terminators
    // (\r, U+2028, U+2029), and group 2 is the js match
    regcomp(&re, "(^|\r|\xe2\x80[\xa8\xa9])(line)", REG_EXTENDED | REG_NEWLINE);
    regmatch_t m[3];

    int64_t total = 0;
    int64_t n = 100000;
    for (int64_t i = 0; i < n; i++) {
        char *s = (char *)malloc(64);
        snprintf(s, 64, "pre\nline-text\nend %" PRId64, i);
        if (regexec(&re, s, 3, m, 0) == 0) {
            char *m0 = strndup(s + m[2].rm_so, (size_t)(m[2].rm_eo - m[2].rm_so));
            total += (int64_t)strlen(m0);
            free(m0);
        }
        free(s);
    }
    regfree(&re);

    printf("%" PRId64 "\n", total);
    return 0;
}
