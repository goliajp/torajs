/*
 * torajs C runtime — string + array helpers that are clearer in C than
 * via the inkwell IR-builder API. Compiled once per `tr build` invoke
 * and linked alongside the generated LLVM IR object.
 *
 * Both heaps follow the same layout the rest of torajs uses:
 *   String = { uint64_t len; uint8_t data[len]; }
 *   Array  = { uint64_t len; uint64_t cap; T data[cap]; }   // T = 8 bytes
 *
 * Forward declarations let us call back into intrinsics that the
 * inkwell side defines (arr_alloc, arr_push). Those resolve at link
 * time inside the same final binary.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/* defined by the inkwell-emitted LLVM IR in the AOT binary */
void *__torajs_arr_alloc(uint64_t initial_cap);
void *__torajs_arr_push(void *arr, int64_t val);

static uint8_t *str_alloc_(uint64_t len) {
    uint8_t *p = (uint8_t *)malloc(8 + (size_t)len);
    *(uint64_t *)p = len;
    return p;
}

void *__torajs_str_split(const uint8_t *s, const uint8_t *sep) {
    uint64_t s_len = *(const uint64_t *)s;
    uint64_t sep_len = *(const uint64_t *)sep;
    const uint8_t *s_data = s + 8;
    const uint8_t *sep_data = sep + 8;

    void *arr = __torajs_arr_alloc(0);

    if (sep_len == 0) {
        /* MVP: empty separator returns [s_clone] (TS char-split is
         * UTF-16-flavored and out of scope for now). */
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(p + 8, s_data, (size_t)s_len);
        return __torajs_arr_push(arr, (int64_t)(intptr_t)p);
    }

    uint64_t start = 0, i = 0;
    while (i + sep_len <= s_len) {
        if (memcmp(s_data + i, sep_data, (size_t)sep_len) == 0) {
            uint64_t seg_len = i - start;
            uint8_t *p = str_alloc_(seg_len);
            if (seg_len) memcpy(p + 8, s_data + start, (size_t)seg_len);
            arr = __torajs_arr_push(arr, (int64_t)(intptr_t)p);
            i += sep_len;
            start = i;
        } else {
            i += 1;
        }
    }
    uint64_t tail_len = s_len - start;
    uint8_t *p = str_alloc_(tail_len);
    if (tail_len) memcpy(p + 8, s_data + start, (size_t)tail_len);
    return __torajs_arr_push(arr, (int64_t)(intptr_t)p);
}

void *__torajs_arr_join(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = *(const uint64_t *)arr;
    uint64_t sep_len = *(const uint64_t *)sep;
    const uint8_t *sep_data = sep + 8;

    if (len == 0) {
        return str_alloc_(0);
    }

    /* pass 1: total = sum(elem.len) + sep_len * (len - 1) */
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        const uint8_t *elem = *(const uint8_t *const *)(arr + 16 + i * 8);
        total += *(const uint64_t *)elem;
    }
    total += sep_len * (len - 1);

    /* pass 2: copy */
    uint8_t *p = str_alloc_(total);
    uint64_t cursor = 8;
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0 && sep_len) {
            memcpy(p + cursor, sep_data, (size_t)sep_len);
            cursor += sep_len;
        }
        const uint8_t *elem = *(const uint8_t *const *)(arr + 16 + i * 8);
        uint64_t elem_len = *(const uint64_t *)elem;
        if (elem_len) {
            memcpy(p + cursor, elem + 8, (size_t)elem_len);
            cursor += elem_len;
        }
    }
    return p;
}
