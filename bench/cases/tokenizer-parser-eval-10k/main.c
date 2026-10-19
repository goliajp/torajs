#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *ptr;
    size_t len;
    size_t cap;
} StrBuf;

static void append(StrBuf *b, const char *s, size_t len) {
    size_t need = b->len + len;
    if (need > b->cap) {
        size_t cap = b->cap ? b->cap : 8;
        while (cap < need) {
            cap *= 2;
        }
        b->ptr = realloc(b->ptr, cap);
        b->cap = cap;
    }
    memcpy(b->ptr + b->len, s, len);
    b->len = need;
}

/* the minimal standard generator; the product stays below 2^47 */
static int64_t next(int64_t s) {
    int64_t t = (s * 48271) % 2147483647;
    return t == 0 ? 1 : t;
}

static int64_t seed = 1;

/* appends left to right, which draws from seed in the order main.ts does */
static void gen(StrBuf *out, int64_t depth) {
    seed = next(seed);
    int64_t r = seed % 6;
    if (depth == 0 || r == 0) {
        seed = next(seed);
        if ((seed & 1) == 0) {
            char digits[24];
            int len = snprintf(digits, sizeof digits, "%" PRId64, seed % 1000);
            append(out, digits, (size_t)len);
            return;
        }
        char c = (char)(97 + ((seed >> 1) & 7));
        append(out, &c, 1);
        return;
    }
    if (r == 1) {
        append(out, "-", 1);
        gen(out, depth - 1);
        return;
    }
    if (r == 2) {
        append(out, "(", 1);
        gen(out, depth - 1);
        append(out, ")", 1);
        return;
    }
    gen(out, depth - 1);
    seed = next(seed);
    int64_t op = seed % 3;
    if (op == 0) {
        append(out, " + ", 3);
    } else if (op == 1) {
        append(out, " - ", 3);
    } else {
        append(out, " * ", 3);
    }
    gen(out, depth - 1);
}

enum { NUM, IDENT, PLUS, MINUS, STAR, LPAREN, RPAREN, END, NEG };

typedef struct {
    int32_t kind;
    int32_t value;
} Token;

typedef struct {
    Token *ptr;
    size_t len;
    size_t cap;
} TokenVec;

static void push_token(TokenVec *v, int32_t kind, int32_t value) {
    if (v->len == v->cap) {
        v->cap = v->cap ? v->cap * 2 : 8;
        v->ptr = realloc(v->ptr, v->cap * sizeof(Token));
    }
    v->ptr[v->len].kind = kind;
    v->ptr[v->len].value = value;
    v->len++;
}

static TokenVec tokenize(const char *src, size_t len) {
    TokenVec out = {NULL, 0, 0};
    size_t i = 0;
    while (i < len) {
        int32_t c = (unsigned char)src[i];
        if (c == 32) {
            i = i + 1;
        } else if (c >= 48 && c <= 57) {
            int32_t v = 0;
            while (i < len) {
                int32_t d = (unsigned char)src[i];
                if (d < 48 || d > 57) {
                    break;
                }
                v = v * 10 + (d - 48);
                i = i + 1;
            }
            push_token(&out, NUM, v);
        } else if (c >= 97 && c <= 122) {
            push_token(&out, IDENT, c - 97);
            i = i + 1;
        } else {
            int32_t kind = RPAREN;
            if (c == 43) {
                kind = PLUS;
            } else if (c == 45) {
                kind = MINUS;
            } else if (c == 42) {
                kind = STAR;
            } else if (c == 40) {
                kind = LPAREN;
            }
            push_token(&out, kind, 0);
            i = i + 1;
        }
    }
    push_token(&out, END, 0);
    return out;
}

typedef struct Expr {
    int32_t kind;
    int32_t value;
    struct Expr *left;
    struct Expr *right;
} Expr;

static Expr *expr_new(int32_t kind, int32_t value, Expr *left, Expr *right) {
    Expr *e = malloc(sizeof(Expr));
    e->kind = kind;
    e->value = value;
    e->left = left;
    e->right = right;
    return e;
}

static void expr_free(Expr *e) {
    if (e == NULL) {
        return;
    }
    expr_free(e->left);
    expr_free(e->right);
    free(e);
}

static int64_t nodes = 0;

typedef struct {
    const Token *toks;
    size_t pos;
} Parser;

static Expr *parse_expr(Parser *p);

static Expr *parse_factor(Parser *p) {
    const Token *t = &p->toks[p->pos];
    p->pos = p->pos + 1;
    nodes = nodes + 1;
    if (t->kind == NUM) {
        return expr_new(NUM, t->value, NULL, NULL);
    }
    if (t->kind == IDENT) {
        return expr_new(IDENT, t->value, NULL, NULL);
    }
    if (t->kind == MINUS) {
        return expr_new(NEG, 0, parse_factor(p), NULL);
    }
    Expr *inner = parse_expr(p);
    p->pos = p->pos + 1;
    nodes = nodes - 1;
    return inner;
}

static Expr *parse_term(Parser *p) {
    Expr *left = parse_factor(p);
    while (p->toks[p->pos].kind == STAR) {
        p->pos = p->pos + 1;
        Expr *right = parse_factor(p);
        nodes = nodes + 1;
        left = expr_new(STAR, 0, left, right);
    }
    return left;
}

static Expr *parse_expr(Parser *p) {
    Expr *left = parse_term(p);
    while (p->toks[p->pos].kind == PLUS || p->toks[p->pos].kind == MINUS) {
        int32_t kind = p->toks[p->pos].kind;
        p->pos = p->pos + 1;
        Expr *right = parse_term(p);
        nodes = nodes + 1;
        left = expr_new(kind, 0, left, right);
    }
    return left;
}

/* | 0 and Math.imul wrap to 32 bits: computed unsigned, read back signed */
static int32_t evaluate(const Expr *e, const int32_t *env) {
    if (e->kind == NUM) {
        return e->value;
    }
    if (e->kind == IDENT) {
        return env[e->value];
    }
    if (e->kind == NEG) {
        return (int32_t)(0u - (uint32_t)evaluate(e->left, env));
    }
    uint32_t a = (uint32_t)evaluate(e->left, env);
    uint32_t b = (uint32_t)evaluate(e->right, env);
    if (e->kind == PLUS) {
        return (int32_t)(a + b);
    }
    if (e->kind == MINUS) {
        return (int32_t)(a - b);
    }
    return (int32_t)(a * b);
}

int main(int argc, char **argv) {
    (void)argc;
    int64_t line_count = strtoll(argv[1], NULL, 10);
    int64_t depth = strtoll(argv[2], NULL, 10);
    int64_t rounds = strtoll(argv[3], NULL, 10);
    seed = strtoll(argv[4], NULL, 10);

    StrBuf *lines = malloc((size_t)line_count * sizeof(StrBuf));
    for (int64_t i = 0; i < line_count; i++) {
        StrBuf line = {NULL, 0, 0};
        gen(&line, depth);
        lines[i] = line;
    }

    int32_t env[8] = {3, 5, 7, 11, 13, 17, 19, 23};
    int32_t total = 0;
    int64_t tokens = 0;
    for (int64_t r = 0; r < rounds; r++) {
        for (int64_t i = 0; i < line_count; i++) {
            TokenVec toks = tokenize(lines[i].ptr, lines[i].len);
            tokens = tokens + (int64_t)toks.len;
            Parser p = {toks.ptr, 0};
            Expr *e = parse_expr(&p);
            total = (int32_t)((uint32_t)total + (uint32_t)evaluate(e, env));
            expr_free(e);
            free(toks.ptr);
        }
        env[r & 7] = (int32_t)((uint32_t)env[r & 7] + (uint32_t)total);
    }
    printf("%" PRId32 " %" PRId64 " %" PRId64 "\n", total, tokens, nodes);

    for (int64_t i = 0; i < line_count; i++) {
        free(lines[i].ptr);
    }
    free(lines);
    return 0;
}
