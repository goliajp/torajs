/* V3-14 — torajs embed C ABI.
 *
 * Link with `libtorajs_embed.a` (static) or `libtorajs_embed.dylib`
 * (shared). Both produced by `cargo build -p torajs-embed`.
 *
 * Example (C):
 *
 *     #include "tora.h"
 *     int main(void) {
 *         return tora_eval_cstr("console.log('hello, embed')");
 *     }
 *
 * Compile + link:
 *     cargo build --release -p torajs-embed
 *     cc -o demo demo.c \
 *         -L target/release \
 *         -ltorajs_embed
 *     ./demo
 */

#ifndef TORAJS_EMBED_H
#define TORAJS_EMBED_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Evaluate `len` bytes of UTF-8 tora source. Returns:
 *    0  — program ran cleanly (or finished with exit 0)
 *    1  — compile-time failure (lex / parse / typecheck / link)
 *    2  — host failure (NULL src, non-UTF-8, subprocess plumbing)
 *  other — propagated exit code from the eval'd program.
 */
int tora_eval(const char *src, size_t len);

/* Convenience: NUL-terminated source string. Same return shape. */
int tora_eval_cstr(const char *src);

#ifdef __cplusplus
}
#endif

#endif /* TORAJS_EMBED_H */
