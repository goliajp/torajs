#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

/* mirrors main.rs: rust never auto-fuses a*b+c into a single rounded FMA
 * without an explicit intrinsic; clang's C default allows intra-expression
 * contraction, so disable it to match rust's rounding exactly. */
#pragma STDC FP_CONTRACT OFF

static uint32_t mandel(double cr, double ci, uint32_t max_iter) {
    double zr = 0.0;
    double zi = 0.0;
    uint32_t n = 0;
    while (n < max_iter) {
        if (zr * zr + zi * zi > 4.0) {
            return n;
        }
        double new_zr = zr * zr - zi * zi + cr;
        zi = 2.0 * zr * zi + ci;
        zr = new_zr;
        n += 1;
    }
    return max_iter;
}

int main(void) {
    uint64_t total = 0;
    for (int i = 0; i < 200; i++) {
        for (int j = 0; j < 200; j++) {
            double cr = (double)i / 100.0 - 1.5;
            double ci = (double)j / 100.0 - 1.0;
            total += (uint64_t)mandel(cr, ci, 1000);
        }
    }
    printf("%" PRIu64 "\n", total);
    return 0;
}
