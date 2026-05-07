/* ============================================================
 * T-21 (v0.6.0) — `fetch(url)` HTTP client (sync MVP).
 *
 * Wraps libcurl's easy interface for a single sync GET. Returns
 * a `Response` heap object with the spec shape:
 *
 *     Response {
 *       header     : 8 bytes (rc + type_tag=RESPONSE + flags)
 *       status     : i64   @ 8       (HTTP status code, 0 on transport error;
 *                                      i64 not i32 so the SSA `.status` field
 *                                      load matches tora's Number ABI without
 *                                      a separate ZExtI32ToI64 path)
 *       body       : Str*  @ 16      (response body, owned)
 *     }
 *     total = 24 bytes
 *
 * SSA-side `fetch(url)` lowers to:
 *     promise_alloc_fulfilled_heap(__torajs_fetch_sync(url))
 * giving `Promise<Response>`. The user awaits it; `.text()`
 * unwraps `body` (already owned), `.status` reads the i32 field.
 *
 * v0.6 MVP scope:
 *   - GET only (no POST / headers / body / method)
 *   - sync (the "real-suspending fetch" lands with T-16's state-
 *     machine async/await, both gated on the same substrate)
 *   - HTTPS via libcurl's bundled OpenSSL/SecureTransport; URL
 *     is whatever libcurl can resolve (system DNS, system trust
 *     store)
 *   - follow-redirects on (matches Bun)
 *   - body returned as a Str (UTF-8 byte sequence — the runtime
 *     doesn't validate; `await response.text()` returns whatever
 *     bytes the server sent)
 *
 * Runtime gating: only compiled into native builds. wasm32-wasi
 * has no curl bindings (per spec it should route through the
 * browser fetch API instead — that's T-21.b).
 * ============================================================ */

#ifndef __wasi__

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <curl/curl.h>

/* Mirror runtime_str.c's universal heap header so we can bump rc /
 * write the type tag without forward-declaring an opaque struct.
 * Layout is binary-compatible (compiler enforces same offsets;
 * any drift fails the `(uint64_t *)` header init pattern). */
typedef struct {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_STR        0
#define __TORAJS_TAG_RESPONSE   9   /* T-21 */

#define __TORAJS_RESPONSE_SIZE       24
#define __TORAJS_RESPONSE_STATUS_OFF 8
#define __TORAJS_RESPONSE_BODY_OFF   16

extern uint8_t *__torajs_str_alloc_pooled(uint64_t len);
extern void __torajs_str_drop(void *p);

/* Curl write callback — accumulates body bytes into a growing
 * heap buffer. Bytes copy in chunks; final size is realloc'd
 * exactly to len at the end so the Str payload is tightly
 * packed. */
struct fetch_buf_ {
    char  *data;
    size_t len;
    size_t cap;
};

static size_t fetch_write_cb_(void *src, size_t s, size_t n, void *ud) {
    struct fetch_buf_ *b = (struct fetch_buf_ *)ud;
    size_t add = s * n;
    size_t need = b->len + add;
    if (need > b->cap) {
        size_t nc = b->cap == 0 ? 4096 : b->cap;
        while (nc < need) nc *= 2;
        char *nd = (char *)realloc(b->data, nc);
        if (nd == NULL) return 0;
        b->data = nd;
        b->cap = nc;
    }
    memcpy(b->data + b->len, src, add);
    b->len += add;
    return add;
}

/* Convert a tora Str* (header + len@8 + bytes@16) to a malloc'd
 * NUL-terminated C string. Caller frees. NULL str → empty C
 * string ("" — libcurl would reject NULL anyway). */
static char *str_to_cstr_(const void *str_ptr) {
    if (str_ptr == NULL) {
        char *e = (char *)malloc(1);
        if (e) e[0] = '\0';
        return e;
    }
    const uint8_t *bytes = (const uint8_t *)str_ptr;
    uint64_t len = *(const uint64_t *)(bytes + 8);
    char *out = (char *)malloc((size_t)len + 1);
    if (out == NULL) return NULL;
    memcpy(out, bytes + 16, (size_t)len);
    out[len] = '\0';
    return out;
}

/* Allocate a Response heap object with rc=1 + type_tag=RESPONSE +
 * status filled + body Str* inserted (caller transfers ownership
 * of `body`). */
static void *response_alloc_(int64_t status, void *body_str_ptr) {
    uint8_t *p = (uint8_t *)malloc(__TORAJS_RESPONSE_SIZE);
    if (p == NULL) return NULL;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    h->refcount = 1;
    h->type_tag = __TORAJS_TAG_RESPONSE;
    h->flags = 0;
    *(int64_t *)(p + __TORAJS_RESPONSE_STATUS_OFF) = status;
    *(void **)(p + __TORAJS_RESPONSE_BODY_OFF) = body_str_ptr;
    return p;
}

/* `fetch(url)` runtime entrypoint. url is a tora Str*. Returns a
 * Response* heap object (rc=1; caller transfers via Promise.value).
 * Transport error (DNS failure / connection refused / TLS reject /
 * etc.) yields status=0 + empty body — surfaces as a clearly-
 * abnormal Response without a separate "throw" path (that's T-21.b
 * once typed-throw fetch lands). */
void *__torajs_fetch_sync(void *url_str_ptr) {
    char *url = str_to_cstr_(url_str_ptr);
    if (url == NULL) {
        uint8_t *empty = __torajs_str_alloc_pooled(0);
        return response_alloc_(0, empty);
    }
    CURL *c = curl_easy_init();
    if (c == NULL) {
        free(url);
        uint8_t *empty = __torajs_str_alloc_pooled(0);
        return response_alloc_(0, empty);
    }
    struct fetch_buf_ b = { NULL, 0, 0 };
    curl_easy_setopt(c, CURLOPT_URL, url);
    curl_easy_setopt(c, CURLOPT_WRITEFUNCTION, fetch_write_cb_);
    curl_easy_setopt(c, CURLOPT_WRITEDATA, &b);
    curl_easy_setopt(c, CURLOPT_FOLLOWLOCATION, 1L);
    /* Bun-parity timeouts. 30s total + 10s connect — long enough
     * for slow pages, short enough to not hang on a dead host. */
    curl_easy_setopt(c, CURLOPT_TIMEOUT, 30L);
    curl_easy_setopt(c, CURLOPT_CONNECTTIMEOUT, 10L);
    /* User-Agent matches `bun` to avoid origins gating on torajs. */
    curl_easy_setopt(c, CURLOPT_USERAGENT, "torajs/0.6 (libcurl)");
    CURLcode rc = curl_easy_perform(c);
    long http_status = 0;
    if (rc == CURLE_OK) {
        curl_easy_getinfo(c, CURLINFO_RESPONSE_CODE, &http_status);
    }
    curl_easy_cleanup(c);
    free(url);
    /* Build the body Str regardless of rc; on transport error b.len
     * is 0 → an empty Str. */
    uint8_t *body = __torajs_str_alloc_pooled((uint64_t)b.len);
    if (body && b.len > 0) {
        memcpy(body + 16, b.data, b.len);
    }
    free(b.data);
    return response_alloc_((int64_t)http_status, body);
}

/* Drop hook — called from runtime_str.c's __torajs_value_drop_heap
 * via the TAG_RESPONSE case. Frees the body Str (rc-aware via the
 * generic value_drop_heap path) then the Response block itself. */
extern int __torajs_rc_dec(void *p);
extern void __torajs_value_drop_heap(void *p);

void __torajs_response_drop(void *p) {
    if (p == NULL) return;
    if (!__torajs_rc_dec(p)) return;
    void *body = *(void **)((uint8_t *)p + __TORAJS_RESPONSE_BODY_OFF);
    if (body) {
        __torajs_value_drop_heap(body);
    }
    free(p);
}

#endif /* !__wasi__ */
