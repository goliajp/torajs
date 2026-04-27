/* AOT host driver — reusable for any tr-emitted wasm module.
 *
 * The pipeline is: tr build → wasm → wasm2c (with --module-name=tora) → clang -O3.
 * wasm2c emits `w2c_tora_*` C symbols. This file pins the WASI fd_write
 * import and runs `_start`, then exits.
 *
 * Compiled together with the wasm2c-generated C and wabt's wasm-rt-impl.c,
 * the resulting native binary runs without any wasm runtime — pure LLVM
 * codegen out the back of the wasm we emit. fib40 lands at ~162 ms here vs
 * ~351 ms via wasmtime/Cranelift.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "tora.h"

u32 w2c_wasi__snapshot__preview1_fd_write(
    struct w2c_wasi__snapshot__preview1* ctx,
    u32 fd, u32 iovs_ptr, u32 iovs_len, u32 nwritten_ptr) {
    /* We pass the module instance itself as the import context (see main()),
     * so we can read its linear memory to gather the iovecs. */
    w2c_tora* m = (w2c_tora*)ctx;
    u8* mem = m->w2c_memory.data;
    u32 total = 0;
    for (u32 i = 0; i < iovs_len; i++) {
        u32 buf_ptr = *(u32*)(mem + iovs_ptr + 8 * i);
        u32 buf_len = *(u32*)(mem + iovs_ptr + 8 * i + 4);
        ssize_t w = write(fd, mem + buf_ptr, buf_len);
        if (w < 0) return 1;
        total += (u32)w;
    }
    *(u32*)(mem + nwritten_ptr) = total;
    return 0;
}

int main(void) {
    wasm_rt_init();
    w2c_tora m;
    /* Pass &m as the wasi import context so fd_write can reach memory. */
    wasm2c_tora_instantiate(&m, (struct w2c_wasi__snapshot__preview1*)&m);
    w2c_tora_0x5Fstart(&m);
    wasm2c_tora_free(&m);
    wasm_rt_free();
    return 0;
}
