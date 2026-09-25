#include <inttypes.h>
#include <setjmp.h>
#include <stdint.h>
#include <stdio.h>

static jmp_buf panic_buf;
static int64_t panic_value;

/* mirrors main.rs: panic::panic_any(i) unwinds the stack up to the nearest
   catch_unwind. C has no built-in unwinding primitive comparable to Rust's
   real (DWARF-table) unwind, so setjmp/longjmp stands in as the closest
   libc equivalent for "jump up the stack, catch higher up" -- this is the
   one place the port cannot be a literal mirror of main.rs's mechanism,
   only of its effect (see report) */
static void do_panic(int64_t i) {
    panic_value = i;
    longjmp(panic_buf, 1);
}

static int64_t trial(int64_t i) {
    if (setjmp(panic_buf) == 0) {
        do_panic(i);
        return 0; /* unreachable: do_panic always longjmps away */
    }
    return panic_value;
}

int main(void) {
    int64_t total = 0;
    for (int64_t i = 0; i < 100000; i++) {
        total = total + trial(i);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
