#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    int64_t id;
    const char *name;
    int64_t score;
    bool active;
} Record;

static Record *make_record(int64_t i) {
    Record *r = malloc(sizeof(Record));
    r->id = i;
    r->name = "row";
    r->score = i * 7;
    r->active = (i & 1) == 0;
    return r;
}

typedef struct {
    char *ptr;
    size_t len;
    size_t cap;
} StrBuf;

/* mirrors main.rs: String growth -- max(2 * cap, needed, 8) */
static void buf_reserve(StrBuf *b, size_t extra) {
    size_t need = b->len + extra;
    if (need <= b->cap) {
        return;
    }
    size_t cap = b->cap * 2;
    if (cap < need) {
        cap = need;
    }
    if (cap < 8) {
        cap = 8;
    }
    b->ptr = realloc(b->ptr, cap);
    b->cap = cap;
}

static void buf_push(StrBuf *b, char c) {
    buf_reserve(b, 1);
    b->ptr[b->len++] = c;
}

static void buf_push_str(StrBuf *b, const char *s, size_t len) {
    buf_reserve(b, len);
    memcpy(b->ptr + b->len, s, len);
    b->len += len;
}

static void buf_push_i64(StrBuf *b, int64_t v) {
    char tmp[20];
    size_t pos = sizeof tmp;
    uint64_t u = v < 0 ? 0 - (uint64_t)v : (uint64_t)v;
    do {
        tmp[--pos] = (char)('0' + u % 10);
        u /= 10;
    } while (u != 0);
    if (v < 0) {
        tmp[--pos] = '-';
    }
    buf_push_str(b, tmp + pos, sizeof tmp - pos);
}

/* QuoteJSONString: escape '"', '\\' and control chars */
static void quote(StrBuf *b, const char *s) {
    buf_push(b, '"');
    for (const unsigned char *p = (const unsigned char *)s; *p != '\0'; p++) {
        unsigned char c = *p;
        switch (c) {
        case '"': buf_push_str(b, "\\\"", 2); break;
        case '\\': buf_push_str(b, "\\\\", 2); break;
        case '\n': buf_push_str(b, "\\n", 2); break;
        case '\r': buf_push_str(b, "\\r", 2); break;
        case '\t': buf_push_str(b, "\\t", 2); break;
        case '\b': buf_push_str(b, "\\b", 2); break;
        case '\f': buf_push_str(b, "\\f", 2); break;
        default:
            if (c < 0x20) {
                char esc[7];
                snprintf(esc, sizeof esc, "\\u%04x", c);
                buf_push_str(b, esc, 6);
            } else {
                buf_push(b, (char)c);
            }
        }
    }
    buf_push(b, '"');
}

/* the key order and value kinds are fixed by the struct; JSON.stringify
   discovers both at run time */
static StrBuf stringify(const Record *r) {
    StrBuf b = {0};
    buf_push(&b, '{');
    quote(&b, "id");
    buf_push(&b, ':');
    buf_push_i64(&b, r->id);
    buf_push(&b, ',');
    quote(&b, "name");
    buf_push(&b, ':');
    quote(&b, r->name);
    buf_push(&b, ',');
    quote(&b, "score");
    buf_push(&b, ':');
    buf_push_i64(&b, r->score);
    buf_push(&b, ',');
    quote(&b, "active");
    buf_push(&b, ':');
    if (r->active) {
        buf_push_str(&b, "true", 4);
    } else {
        buf_push_str(&b, "false", 5);
    }
    buf_push(&b, '}');
    return b;
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t total = 0;
    int64_t n = strtoll(argv[1], NULL, 10);
    for (int64_t i = 0; i < n; i++) {
        Record *r = make_record(i);
        // mirrors main.rs: black_box keeps the record allocation
        __asm__ volatile("" : "+r"(r) : : "memory");
        StrBuf s = stringify(r);
        total = total + (int64_t)s.len;
        free(s.ptr);
        free(r);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
