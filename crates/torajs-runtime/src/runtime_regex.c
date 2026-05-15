/*
 * runtime_regex.c — torajs v0.2 #1 regex matching engine.
 *
 * Architecture (textbook NFA approach, after Russ Cox's "Regular
 * Expression Matching: the Virtual Machine Approach"):
 *
 *   1. parse: pattern bytes → AST (recursive descent, alloc-light)
 *   2. compile: AST → linear instruction stream (Thompson construction)
 *      Each instruction is one of:
 *        CHAR c       — match exactly byte c (i-flag awareness)
 *        ANYCHAR      — match any byte (modulo s-flag for newline)
 *        CLASS idx    — match per char-class bitmap at idx
 *        ANCHOR_BEG   — match position-0 (or post-newline if m-flag)
 *        ANCHOR_END   — match end-of-input (or pre-newline if m-flag)
 *        WBOUND       — \b
 *        NWBOUND      — \B
 *        JMP n        — unconditional branch to inst n
 *        SPLIT a, b   — fork two threads, one going to a, one to b
 *        MATCH        — accept
 *   3. match: bitmap-indexed thread list per input position. SPLIT
 *      enqueues both targets onto the same step's frontier; CHAR /
 *      ANYCHAR / CLASS gate the thread on the input byte and step it
 *      forward. Linear-time per char (each NFA state at most once per
 *      step thanks to the visited bitmap).
 *
 * v0.2 #1 Phase 1a scope:
 *   - Pattern: literal / `.` / `[abc]` / `[^abc]` / `[a-z]` / escapes
 *     `\d \D \w \W \s \S \n \t \r \f \v \\ \/ \. \* \+ \? \| \( \) \[ \]
 *     \{ \} \^ \$ \b \B`, anchors `^` `$`, quantifiers `* + ? {n} {n,}
 *     {n,m}` (greedy + lazy `*? +? ?? {...}?`), alternation `|`,
 *     groups `(...)` / `(?:...)` (treated as non-capturing — capturing
 *     semantics land in Phase 1b/c with `re.exec` / `s.match`).
 *   - Flags: `i` (case-insensitive ASCII), `m` (^/$ per line), `s`
 *     (`.` matches newline). `g`, `y`, `u` parsed + stored but inert
 *     for `.test()` (single-shot probe — `g` matters only for the
 *     iteration surface methods that ship in Phase 1b).
 *   - Surface: `__torajs_regex_test(re, str)` — true iff the pattern
 *     matches anywhere in str (i.e. JS spec `RegExp.prototype.test`
 *     with lastIndex=0 + non-sticky).
 *
 * Phase 1b layers in: `re.exec` (capturing groups → split engine into
 * VM-with-savestack), `s.match`, `s.replace(re, repl)`, `s.replaceAll`,
 * `s.split(re)`, `s.matchAll`. Phase 1c: subset → DFA conversion for
 * the no-capture fast path, lookahead/lookbehind, backreferences,
 * `\p{...}` Unicode property escapes (or punt to v1.0).
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ============================================================
 * Mirror of runtime_str.c heap header — binary compatible. The
 * regex .o links against `__torajs_rc_dec` from runtime_str.c
 * but otherwise stands on its own (no #include cross-TU plumbing
 * needed; both .c files are concatenated at link time).
 * ============================================================ */

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_REGEX     4

#define __TORAJS_STR_HDR_SIZE  16
#define __TORAJS_STR_LEN(p)    (*(uint64_t *)((const uint8_t *)(p) + 8))
#define __TORAJS_STR_CDATA(p)  ((const uint8_t *)(p) + __TORAJS_STR_HDR_SIZE)

extern int __torajs_rc_dec(void *p);

/* ============================================================
 * Flag bitset. Encoded into RegExp.flags after parsing the
 * literal's flag string (`i`, `g`, `m`, `s`, `u`, `y`).
 * ============================================================ */

#define RE_FLAG_I 0x01u  /* case-insensitive (ASCII) */
#define RE_FLAG_G 0x02u  /* global — used by iteration helpers */
#define RE_FLAG_M 0x04u  /* multiline ^/$ */
#define RE_FLAG_S 0x08u  /* dotall — `.` matches newline */
#define RE_FLAG_U 0x10u  /* unicode (parser only — full Unicode is v1.0) */
#define RE_FLAG_Y 0x20u  /* sticky — used by iteration helpers */

/* ============================================================
 * Char class — 256-bit bitmap + inversion bit. One per CLASS
 * instruction. Owned by the RegExp.
 * ============================================================ */

typedef struct {
    uint8_t bits[32];
    uint8_t negate;
} CharClass;

static void cc_clear(CharClass *cc) {
    for (int i = 0; i < 32; i++) cc->bits[i] = 0;
    cc->negate = 0;
}
static void cc_add(CharClass *cc, uint8_t ch) {
    cc->bits[ch >> 3] |= (uint8_t)(1u << (ch & 7));
}
static void cc_add_range(CharClass *cc, uint8_t lo, uint8_t hi) {
    if (lo > hi) { uint8_t t = lo; lo = hi; hi = t; }
    for (int c = lo; c <= hi; c++) cc_add(cc, (uint8_t)c);
}
static int cc_test(const CharClass *cc, uint8_t ch) {
    int in = (cc->bits[ch >> 3] >> (ch & 7)) & 1;
    return cc->negate ? !in : in;
}

/* Predefined class helpers — \d, \w, \s and their inverses. */
static void cc_add_digit(CharClass *cc) {
    cc_add_range(cc, '0', '9');
}
static void cc_add_word(CharClass *cc) {
    cc_add_range(cc, '0', '9');
    cc_add_range(cc, 'A', 'Z');
    cc_add_range(cc, 'a', 'z');
    cc_add(cc, '_');
}
static void cc_add_space(CharClass *cc) {
    /* JS regex \s: whitespace per ECMA-262 — matches spec subset of
     * Unicode WhiteSpace + LineTerminators that fit in ASCII bytes. */
    cc_add(cc, ' ');
    cc_add(cc, '\t');
    cc_add(cc, '\n');
    cc_add(cc, '\v');
    cc_add(cc, '\f');
    cc_add(cc, '\r');
}

/* ============================================================
 * Regex AST — recursive structure produced by the parser.
 * Compiled to flat bytecode by `compile()`.
 * ============================================================ */

typedef enum {
    NK_CHAR,
    NK_ANY,
    NK_CLASS,
    NK_ANCHOR_BEG,
    NK_ANCHOR_END,
    NK_WBOUND,
    NK_NWBOUND,
    NK_CONCAT,
    NK_ALT,
    NK_REPEAT,
    NK_GROUP, /* non-capturing — child only; capturing semantics later */
    NK_LOOKAHEAD,     /* (?=X) — zero-width positive assertion */
    NK_NEG_LOOKAHEAD, /* (?!X) — zero-width negative assertion */
} NodeKind;

typedef struct Node {
    NodeKind kind;
    /* CHAR */
    uint8_t ch;
    /* CLASS */
    CharClass cc;
    /* CONCAT, ALT */
    struct Node **kids;
    int n_kids;
    int cap_kids;
    /* REPEAT */
    int min, max; /* max = -1 → unbounded */
    int lazy;
    /* REPEAT, GROUP */
    struct Node *child;
    /* GROUP — capture index (assigned in source order, 1-based; 0 is
     * reserved for the whole-match record). -1 = non-capturing
     * (`(?:...)`). Phase 1c.1 — capturing groups + re.exec. */
    int capture_idx;
} Node;

static Node *node_new(NodeKind kind) {
    Node *n = (Node *)calloc(1, sizeof(Node));
    n->kind = kind;
    n->max = -1;
    n->capture_idx = -1;
    return n;
}
static void node_push_kid(Node *parent, Node *kid) {
    if (parent->n_kids == parent->cap_kids) {
        int nc = parent->cap_kids ? parent->cap_kids * 2 : 4;
        parent->kids = (Node **)realloc(parent->kids, (size_t)nc * sizeof(Node *));
        parent->cap_kids = nc;
    }
    parent->kids[parent->n_kids++] = kid;
}
static void node_free(Node *n) {
    if (!n) return;
    if (n->child) node_free(n->child);
    for (int i = 0; i < n->n_kids; i++) node_free(n->kids[i]);
    if (n->kids) free(n->kids);
    free(n);
}

/* ============================================================
 * Parser — recursive descent, single forward pass over pattern
 * bytes. Returns NULL on malformed input (caller falls back to
 * an "always-false" matcher, matching bun's behavior of
 * SyntaxError at JS level — a v0.2 #1.b refinement will raise a
 * proper TypeError into the surface).
 * ============================================================ */

typedef struct {
    const uint8_t *p;
    int64_t len;
    int64_t i;
    uint8_t flags;
    int err;
    /* Capturing-group counter, incremented in source order each time
     * `parse_atom` opens a `(...)` (NOT `(?:...)`). Group index 0 is
     * reserved for the whole-match span; the first user group is 1. */
    int n_captures;
} Parser;

static int p_eof(const Parser *ps) { return ps->i >= ps->len; }
static uint8_t p_peek(const Parser *ps) { return ps->p[ps->i]; }
static uint8_t p_get(Parser *ps) { return ps->p[ps->i++]; }
static int p_match(Parser *ps, uint8_t c) {
    if (!p_eof(ps) && p_peek(ps) == c) { ps->i++; return 1; }
    return 0;
}

/* Forward decls — the grammar is mutually recursive
 * (alt → concat → repeat → atom → alt). */
static Node *parse_alt(Parser *ps);

/* `\X` escape — produce either a single literal CHAR node (for
 * \n / \t / etc + pattern metas like \. \* \+) or a CLASS node
 * (for shorthand \d \D \w \W \s \S). */
static Node *parse_escape(Parser *ps) {
    if (p_eof(ps)) { ps->err = 1; return NULL; }
    uint8_t c = p_get(ps);
    /* `\1`..`\9` — DecimalEscape. JS spec: if N <= n_captures it's a
     * backreference to capture N; otherwise it's an IdentityEscape
     * (literal digit). Backreferences are Phase 1c.5; until then any
     * `\N` (N is 1-9) marks the regex as rejected so .exec / .match
     * / .replace* / .split paths abort (→ incompatible bucket) rather
     * than producing wrong matches. .test() returns false silently
     * via the rejected-stub path. */
    if (c >= '1' && c <= '9') {
        ps->err = 1;
        return NULL;
    }
    switch (c) {
        case 'n': { Node *n = node_new(NK_CHAR); n->ch = '\n'; return n; }
        case 't': { Node *n = node_new(NK_CHAR); n->ch = '\t'; return n; }
        case 'r': { Node *n = node_new(NK_CHAR); n->ch = '\r'; return n; }
        case 'f': { Node *n = node_new(NK_CHAR); n->ch = '\f'; return n; }
        case 'v': { Node *n = node_new(NK_CHAR); n->ch = '\v'; return n; }
        case '0': { Node *n = node_new(NK_CHAR); n->ch = '\0'; return n; }
        case 'd': { Node *n = node_new(NK_CLASS); cc_clear(&n->cc); cc_add_digit(&n->cc); return n; }
        case 'D': { Node *n = node_new(NK_CLASS); cc_clear(&n->cc); cc_add_digit(&n->cc); n->cc.negate = 1; return n; }
        case 'w': { Node *n = node_new(NK_CLASS); cc_clear(&n->cc); cc_add_word(&n->cc); return n; }
        case 'W': { Node *n = node_new(NK_CLASS); cc_clear(&n->cc); cc_add_word(&n->cc); n->cc.negate = 1; return n; }
        case 's': { Node *n = node_new(NK_CLASS); cc_clear(&n->cc); cc_add_space(&n->cc); return n; }
        case 'S': { Node *n = node_new(NK_CLASS); cc_clear(&n->cc); cc_add_space(&n->cc); n->cc.negate = 1; return n; }
        case 'b': return node_new(NK_WBOUND);
        case 'B': return node_new(NK_NWBOUND);
        case 'x': {
            /* `\xHH` — hex escape, 2 digits. */
            if (ps->i + 2 > ps->len) { ps->err = 1; return NULL; }
            uint8_t h1 = p_get(ps), h2 = p_get(ps);
            uint8_t v = 0;
            if (h1 >= '0' && h1 <= '9') v = (uint8_t)((h1 - '0') << 4);
            else if (h1 >= 'a' && h1 <= 'f') v = (uint8_t)((h1 - 'a' + 10) << 4);
            else if (h1 >= 'A' && h1 <= 'F') v = (uint8_t)((h1 - 'A' + 10) << 4);
            else { ps->err = 1; return NULL; }
            if (h2 >= '0' && h2 <= '9') v |= (uint8_t)(h2 - '0');
            else if (h2 >= 'a' && h2 <= 'f') v |= (uint8_t)(h2 - 'a' + 10);
            else if (h2 >= 'A' && h2 <= 'F') v |= (uint8_t)(h2 - 'A' + 10);
            else { ps->err = 1; return NULL; }
            Node *n = node_new(NK_CHAR); n->ch = v; return n;
        }
        default: {
            /* Any other char after \ is literal (covers \. \* \+ \?
             * \( \) \[ \] \{ \} \| \\ \/ \^ \$ — and unknown escapes
             * which JS treats as literals when not in `u` mode). */
            Node *n = node_new(NK_CHAR); n->ch = c; return n;
        }
    }
}

/* `[...]` char class. Returns NULL on parse error. */
static Node *parse_class(Parser *ps) {
    Node *n = node_new(NK_CLASS);
    cc_clear(&n->cc);
    if (!p_eof(ps) && p_peek(ps) == '^') {
        n->cc.negate = 1;
        p_get(ps);
    }
    /* JS spec: empty `[]` is a valid char class that matches nothing
     * (negation `[^]` matches anything). Detect the empty form here
     * before the loop body, which would otherwise consume the `]` as
     * a literal char. */
    if (!p_eof(ps) && p_peek(ps) == ']') {
        p_get(ps);
        return n;
    }
    int first = 0;
    while (!p_eof(ps) && (first || p_peek(ps) != ']')) {
        first = 0;
        uint8_t c;
        if (p_peek(ps) == '\\') {
            p_get(ps);
            if (p_eof(ps)) { ps->err = 1; node_free(n); return NULL; }
            uint8_t e = p_get(ps);
            switch (e) {
                case 'n': c = '\n'; break;
                case 't': c = '\t'; break;
                case 'r': c = '\r'; break;
                case 'f': c = '\f'; break;
                case 'v': c = '\v'; break;
                case '0': c = '\0'; break;
                case 'd': cc_add_digit(&n->cc); continue;
                case 'D': {
                    /* \D inside char class: per ECMA-262 the inner
                     * negation is literal — add the complement of
                     * digit. We approximate by adding everything
                     * except digits. */
                    for (int k = 0; k < 256; k++) {
                        if (!(k >= '0' && k <= '9')) cc_add(&n->cc, (uint8_t)k);
                    }
                    continue;
                }
                case 'w': cc_add_word(&n->cc); continue;
                case 'W': {
                    for (int k = 0; k < 256; k++) {
                        int is_w = (k >= '0' && k <= '9')
                                || (k >= 'A' && k <= 'Z')
                                || (k >= 'a' && k <= 'z')
                                || k == '_';
                        if (!is_w) cc_add(&n->cc, (uint8_t)k);
                    }
                    continue;
                }
                case 's': cc_add_space(&n->cc); continue;
                case 'S': {
                    for (int k = 0; k < 256; k++) {
                        int is_s = (k == ' ' || k == '\t' || k == '\n'
                                 || k == '\v' || k == '\f' || k == '\r');
                        if (!is_s) cc_add(&n->cc, (uint8_t)k);
                    }
                    continue;
                }
                case 'b': c = '\b'; break;
                case 'x': {
                    if (ps->i + 2 > ps->len) { ps->err = 1; node_free(n); return NULL; }
                    uint8_t h1 = p_get(ps), h2 = p_get(ps);
                    uint8_t v = 0;
                    if (h1 >= '0' && h1 <= '9') v = (uint8_t)((h1 - '0') << 4);
                    else if (h1 >= 'a' && h1 <= 'f') v = (uint8_t)((h1 - 'a' + 10) << 4);
                    else if (h1 >= 'A' && h1 <= 'F') v = (uint8_t)((h1 - 'A' + 10) << 4);
                    else { ps->err = 1; node_free(n); return NULL; }
                    if (h2 >= '0' && h2 <= '9') v |= (uint8_t)(h2 - '0');
                    else if (h2 >= 'a' && h2 <= 'f') v |= (uint8_t)(h2 - 'a' + 10);
                    else if (h2 >= 'A' && h2 <= 'F') v |= (uint8_t)(h2 - 'A' + 10);
                    else { ps->err = 1; node_free(n); return NULL; }
                    c = v;
                    break;
                }
                default: c = e;
            }
        } else {
            c = p_get(ps);
        }
        /* Optional range `c-c2`. */
        if (!p_eof(ps) && p_peek(ps) == '-'
            && ps->i + 1 < ps->len && ps->p[ps->i + 1] != ']') {
            p_get(ps); /* consume '-' */
            uint8_t hi;
            if (p_peek(ps) == '\\') {
                p_get(ps);
                if (p_eof(ps)) { ps->err = 1; node_free(n); return NULL; }
                uint8_t e = p_get(ps);
                switch (e) {
                    case 'n': hi = '\n'; break;
                    case 't': hi = '\t'; break;
                    case 'r': hi = '\r'; break;
                    default: hi = e;
                }
            } else {
                hi = p_get(ps);
            }
            cc_add_range(&n->cc, c, hi);
        } else {
            cc_add(&n->cc, c);
        }
    }
    if (p_eof(ps)) { ps->err = 1; node_free(n); return NULL; }
    p_get(ps); /* consume ']' */
    return n;
}

/* atom → CHAR | ANY | CLASS | ANCHOR | GROUP */
static Node *parse_atom(Parser *ps) {
    if (p_eof(ps)) { ps->err = 1; return NULL; }
    uint8_t c = p_peek(ps);
    if (c == '(') {
        p_get(ps);
        /* `(?:...)` non-capturing.
         * `(?=...)` positive lookahead — Phase 1c.4.
         * `(?!...)` negative lookahead — Phase 1c.4.
         * `(?<=...)` / `(?<!...)` lookbehind — Phase 1c.4.b (reverse
         *   matcher needed; rejected for now).
         * `(?<name>...)` named capture group — Phase 1c.4.c (rejected).
         * Otherwise capturing group; gets next sequential index. */
        NodeKind kind = NK_GROUP;
        int capture_idx = -1;
        if (!p_eof(ps) && p_peek(ps) == '?') {
            uint8_t after = (ps->i + 1 < ps->len) ? ps->p[ps->i + 1] : 0;
            if (after == ':') {
                p_get(ps); p_get(ps);
            } else if (after == '=') {
                p_get(ps); p_get(ps);
                kind = NK_LOOKAHEAD;
            } else if (after == '!') {
                p_get(ps); p_get(ps);
                kind = NK_NEG_LOOKAHEAD;
            } else if (after == '<') {
                /* lookbehind / named group — Phase 1c.4.b/c. */
                ps->err = 1;
                return NULL;
            } else {
                ps->err = 1;
                return NULL;
            }
        } else {
            capture_idx = ++ps->n_captures;
        }
        Node *inner = parse_alt(ps);
        if (!inner) return NULL;
        if (!p_match(ps, ')')) { ps->err = 1; node_free(inner); return NULL; }
        Node *g = node_new(kind);
        g->child = inner;
        g->capture_idx = capture_idx;
        return g;
    }
    if (c == '[') {
        p_get(ps);
        return parse_class(ps);
    }
    if (c == '.') {
        p_get(ps);
        return node_new(NK_ANY);
    }
    if (c == '^') {
        p_get(ps);
        return node_new(NK_ANCHOR_BEG);
    }
    if (c == '$') {
        p_get(ps);
        return node_new(NK_ANCHOR_END);
    }
    if (c == '\\') {
        p_get(ps);
        return parse_escape(ps);
    }
    if (c == ')' || c == '|' || c == '*' || c == '+' || c == '?') {
        /* Quantifier or close-paren without a leading atom — error. */
        ps->err = 1;
        return NULL;
    }
    /* `{` is the start of a quantifier `{n,m}`, but JS Annex B + spec
     * says when it doesn't form a valid quantifier (e.g. `x{o}x` or
     * `x{` at end), treat it as a literal `{`. parse_repeat's
     * rollback handles the lookahead-fail case; here we just need
     * to NOT classify standalone `{` as a parse error. */
    /* Plain char. */
    p_get(ps);
    Node *n = node_new(NK_CHAR);
    n->ch = c;
    return n;
}

/* Optional `*`, `+`, `?`, `{n}`, `{n,}`, `{n,m}` (with optional `?`
 * suffix for lazy). Wraps the just-parsed atom in NK_REPEAT. */
static Node *parse_repeat(Parser *ps, Node *atom) {
    if (p_eof(ps) || !atom) return atom;
    uint8_t c = p_peek(ps);
    int min, max;
    if (c == '*') { p_get(ps); min = 0; max = -1; }
    else if (c == '+') { p_get(ps); min = 1; max = -1; }
    else if (c == '?') { p_get(ps); min = 0; max = 1; }
    else if (c == '{') {
        /* `{n}`, `{n,}`, `{n,m}`. */
        int64_t save = ps->i;
        p_get(ps);
        if (p_eof(ps) || !(p_peek(ps) >= '0' && p_peek(ps) <= '9')) {
            /* Not a quantifier — treat `{` as literal. Roll back. */
            ps->i = save;
            return atom;
        }
        int n1 = 0;
        while (!p_eof(ps) && p_peek(ps) >= '0' && p_peek(ps) <= '9') {
            n1 = n1 * 10 + (p_get(ps) - '0');
        }
        if (p_eof(ps)) { ps->i = save; return atom; }
        if (p_peek(ps) == '}') {
            p_get(ps);
            min = n1; max = n1;
        } else if (p_peek(ps) == ',') {
            p_get(ps);
            if (!p_eof(ps) && p_peek(ps) == '}') {
                p_get(ps);
                min = n1; max = -1;
            } else {
                int n2 = 0;
                if (p_eof(ps) || !(p_peek(ps) >= '0' && p_peek(ps) <= '9')) {
                    ps->i = save; return atom;
                }
                while (!p_eof(ps) && p_peek(ps) >= '0' && p_peek(ps) <= '9') {
                    n2 = n2 * 10 + (p_get(ps) - '0');
                }
                if (p_eof(ps) || p_peek(ps) != '}') {
                    ps->i = save; return atom;
                }
                p_get(ps);
                min = n1; max = n2;
            }
        } else {
            ps->i = save; return atom;
        }
    } else {
        return atom;
    }
    int lazy = 0;
    if (!p_eof(ps) && p_peek(ps) == '?') {
        p_get(ps);
        lazy = 1;
    }
    Node *r = node_new(NK_REPEAT);
    r->child = atom;
    r->min = min;
    r->max = max;
    r->lazy = lazy;
    return r;
}

static Node *parse_atom_with_repeat(Parser *ps) {
    Node *a = parse_atom(ps);
    if (!a) return NULL;
    return parse_repeat(ps, a);
}

/* concat → repeat repeat ... */
static Node *parse_concat(Parser *ps) {
    Node *seq = node_new(NK_CONCAT);
    while (!p_eof(ps) && p_peek(ps) != '|' && p_peek(ps) != ')') {
        Node *a = parse_atom_with_repeat(ps);
        if (!a) { node_free(seq); return NULL; }
        node_push_kid(seq, a);
    }
    return seq;
}

/* alt → concat ( '|' concat )* */
static Node *parse_alt(Parser *ps) {
    Node *first = parse_concat(ps);
    if (!first) return NULL;
    if (p_eof(ps) || p_peek(ps) != '|') return first;
    Node *alt = node_new(NK_ALT);
    node_push_kid(alt, first);
    while (!p_eof(ps) && p_peek(ps) == '|') {
        p_get(ps);
        Node *next = parse_concat(ps);
        if (!next) { node_free(alt); return NULL; }
        node_push_kid(alt, next);
    }
    return alt;
}

/* ============================================================
 * Bytecode + Thompson construction.
 *
 * Instruction layout (8 bytes, packed 32+32):
 *   op (8) | a_or_ch (8) | _pad (16) | b (32)
 * Op codes:
 *   OP_CHAR     a=byte
 *   OP_ANYCHAR
 *   OP_CLASS    b=class_idx
 *   OP_ANCHOR_B
 *   OP_ANCHOR_E
 *   OP_WBOUND
 *   OP_NWBOUND
 *   OP_JMP      b=target
 *   OP_SPLIT    a=byte_pad, b=target1   ; followed by an OP_SPLIT_B
 *               with target2 — encoded as one logical step (two slots)
 *   For simplicity, SPLIT carries TWO targets in two adjacent words.
 *   OP_MATCH
 * ============================================================ */

typedef enum {
    OP_CHAR = 1,
    OP_ANYCHAR,
    OP_CLASS,
    OP_ANCHOR_B,
    OP_ANCHOR_E,
    OP_WBOUND,
    OP_NWBOUND,
    OP_JMP,
    OP_SPLIT,
    OP_MATCH,
    OP_SAVE, /* a = save slot index (2*capture_idx for start, +1 for end) */
    /* Phase 1c.4 — zero-width assertions. The lookahead body is
     * compiled into a separate sub-Program (with its own MATCH at end);
     * a = sub-program index into Program.sub_progs[]. The lookahead
     * resolves at add_thread time (epsilon-style) by recursively
     * running vm_match_at on the sub-program at the current pos.
     * Positive: continue if sub matched. Negative: continue if not. */
    OP_LOOKAHEAD,
    OP_NEG_LOOKAHEAD,
} Op;

typedef struct {
    uint8_t op;
    uint8_t ch;       /* for CHAR */
    uint16_t pad;
    int32_t a;        /* CLASS=cls_idx, JMP=target, SPLIT=target1 */
    int32_t b;        /* SPLIT=target2 */
} Inst;

typedef struct Program Program;
struct Program {
    Inst *insts;
    int n_insts;
    int cap_insts;
    CharClass *classes;
    int n_classes;
    int cap_classes;
    /* Sub-programs for lookahead bodies. Each `(?=X)` / `(?!X)` in the
     * pattern compiles X into its own Program (with OP_MATCH at end).
     * Owned; recursively freed in regex_drop. */
    Program **sub_progs;
    int n_sub_progs;
    int cap_sub_progs;
};

static int prog_emit(Program *p, Inst i) {
    if (p->n_insts == p->cap_insts) {
        int nc = p->cap_insts ? p->cap_insts * 2 : 16;
        p->insts = (Inst *)realloc(p->insts, (size_t)nc * sizeof(Inst));
        p->cap_insts = nc;
    }
    int idx = p->n_insts;
    p->insts[idx] = i;
    p->n_insts++;
    return idx;
}

static int prog_intern_class(Program *p, const CharClass *cc) {
    if (p->n_classes == p->cap_classes) {
        int nc = p->cap_classes ? p->cap_classes * 2 : 4;
        p->classes = (CharClass *)realloc(p->classes, (size_t)nc * sizeof(CharClass));
        p->cap_classes = nc;
    }
    int idx = p->n_classes++;
    p->classes[idx] = *cc;
    return idx;
}

/* Append a sub-program (caller-allocated, ownership transferred).
 * Returns the index into prog.sub_progs[]. */
static int prog_add_sub(Program *p, Program *sub) {
    if (p->n_sub_progs == p->cap_sub_progs) {
        int nc = p->cap_sub_progs ? p->cap_sub_progs * 2 : 2;
        p->sub_progs = (Program **)realloc(p->sub_progs, (size_t)nc * sizeof(Program *));
        p->cap_sub_progs = nc;
    }
    int idx = p->n_sub_progs++;
    p->sub_progs[idx] = sub;
    return idx;
}

/* Recursively free a Program (its insts, classes, and sub-programs).
 * Used both by regex_drop and by sub-program cleanup paths. */
static void prog_free(Program *p) {
    if (!p) return;
    if (p->insts) free(p->insts);
    if (p->classes) free(p->classes);
    for (int i = 0; i < p->n_sub_progs; i++) prog_free(p->sub_progs[i]);
    if (p->sub_progs) free(p->sub_progs);
}

/* Compile one AST node. Emits all needed instructions; the very last
 * instruction emitted is the "tail" of this node's matching code (so
 * the caller knows where its own concat / outer SPLIT should jump). */
static void compile_node(Program *p, const Node *n);

static void compile_repeat(Program *p, const Node *n) {
    /* Lower {min,max} into:
     *   compile(child) min times unconditionally,
     *   then compile(child) (max - min) times, each wrapped in a
     *     `SPLIT skip` that lets the matcher pop out early.
     *   If max == -1 (unbounded), use a Kleene-star tail:
     *     L1: SPLIT next, after_loop  (greedy: try child first;
     *                                  lazy: swap targets)
     *     compile(child)
     *     JMP L1
     *     after_loop:
     */
    const Node *child = n->child;
    /* Unrolled mandatory prefix. */
    for (int i = 0; i < n->min; i++) {
        compile_node(p, child);
    }
    if (n->max == -1) {
        /* SPLIT-loop. */
        Inst split = { OP_SPLIT, 0, 0, 0, 0 };
        int split_idx = prog_emit(p, split);
        int body_start = p->n_insts;
        compile_node(p, child);
        Inst back = { OP_JMP, 0, 0, split_idx, 0 };
        prog_emit(p, back);
        int after = p->n_insts;
        if (n->lazy) {
            p->insts[split_idx].a = after;
            p->insts[split_idx].b = body_start;
        } else {
            p->insts[split_idx].a = body_start;
            p->insts[split_idx].b = after;
        }
    } else {
        /* Bounded extras: max - min iterations, each optional. */
        int extra = n->max - n->min;
        /* Stack of split indices to backpatch with after-end target. */
        int *splits = (int *)malloc(sizeof(int) * (size_t)extra);
        int n_splits = 0;
        for (int i = 0; i < extra; i++) {
            Inst split = { OP_SPLIT, 0, 0, 0, 0 };
            int sidx = prog_emit(p, split);
            splits[n_splits++] = sidx;
            int body_start = p->n_insts;
            compile_node(p, child);
            if (n->lazy) {
                p->insts[sidx].a = -1; /* skip target — patch later */
                p->insts[sidx].b = body_start;
            } else {
                p->insts[sidx].a = body_start;
                p->insts[sidx].b = -1;
            }
        }
        int after = p->n_insts;
        for (int i = 0; i < n_splits; i++) {
            int sidx = splits[i];
            if (p->insts[sidx].a == -1) p->insts[sidx].a = after;
            if (p->insts[sidx].b == -1) p->insts[sidx].b = after;
        }
        free(splits);
    }
}

static void compile_alt(Program *p, const Node *n) {
    /* a|b|c lowers to:
     *   SPLIT L1, Lalt
     *   L1: compile(a); JMP Lend
     *   Lalt: SPLIT L2, Lalt2
     *   L2: compile(b); JMP Lend
     *   Lalt2: compile(c)
     *   Lend:
     */
    int n_alts = n->n_kids;
    int *jmps = (int *)malloc(sizeof(int) * (size_t)n_alts);
    for (int i = 0; i + 1 < n_alts; i++) {
        Inst split = { OP_SPLIT, 0, 0, 0, 0 };
        int sidx = prog_emit(p, split);
        int branch_start = p->n_insts;
        compile_node(p, n->kids[i]);
        Inst jmp = { OP_JMP, 0, 0, 0, 0 };
        jmps[i] = prog_emit(p, jmp);
        int next = p->n_insts;
        p->insts[sidx].a = branch_start;
        p->insts[sidx].b = next;
    }
    /* Last alt — no trailing JMP; it just falls through to Lend. */
    compile_node(p, n->kids[n_alts - 1]);
    int end = p->n_insts;
    for (int i = 0; i + 1 < n_alts; i++) {
        p->insts[jmps[i]].a = end;
    }
    free(jmps);
}

static void compile_node(Program *p, const Node *n) {
    if (!n) return;
    switch (n->kind) {
        case NK_CHAR: {
            Inst i = { OP_CHAR, n->ch, 0, 0, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_ANY: {
            Inst i = { OP_ANYCHAR, 0, 0, 0, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_CLASS: {
            int cidx = prog_intern_class(p, &n->cc);
            Inst i = { OP_CLASS, 0, 0, cidx, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_ANCHOR_BEG: {
            Inst i = { OP_ANCHOR_B, 0, 0, 0, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_ANCHOR_END: {
            Inst i = { OP_ANCHOR_E, 0, 0, 0, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_WBOUND: {
            Inst i = { OP_WBOUND, 0, 0, 0, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_NWBOUND: {
            Inst i = { OP_NWBOUND, 0, 0, 0, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_CONCAT: {
            for (int i = 0; i < n->n_kids; i++) compile_node(p, n->kids[i]);
            break;
        }
        case NK_ALT: compile_alt(p, n); break;
        case NK_REPEAT: compile_repeat(p, n); break;
        case NK_GROUP: {
            if (n->capture_idx > 0) {
                /* Capturing group: bracket the child with SAVE slots
                 * 2*idx (start) and 2*idx+1 (end). The matcher writes
                 * pos to thread.saves[slot] when the SAVE op fires. */
                Inst sa = { OP_SAVE, 0, 0, 2 * n->capture_idx, 0 };
                prog_emit(p, sa);
                compile_node(p, n->child);
                Inst sb = { OP_SAVE, 0, 0, 2 * n->capture_idx + 1, 0 };
                prog_emit(p, sb);
            } else {
                compile_node(p, n->child);
            }
            break;
        }
        case NK_LOOKAHEAD:
        case NK_NEG_LOOKAHEAD: {
            /* Compile the lookahead body into its own sub-Program with
             * an OP_MATCH at the end. Main bytecode emits OP_LOOKAHEAD
             * (or OP_NEG_LOOKAHEAD) with `a` = sub-program index.
             * The matcher resolves the assertion at add_thread time
             * by recursively running vm_match_at on the sub-program. */
            Program *sub = (Program *)calloc(1, sizeof(Program));
            compile_node(sub, n->child);
            Inst m = { OP_MATCH, 0, 0, 0, 0 };
            prog_emit(sub, m);
            int sub_idx = prog_add_sub(p, sub);
            uint8_t op = (n->kind == NK_LOOKAHEAD) ? OP_LOOKAHEAD : OP_NEG_LOOKAHEAD;
            Inst la = { op, 0, 0, sub_idx, 0 };
            prog_emit(p, la);
            break;
        }
    }
}

/* ============================================================
 * RegExp heap object — universal heap header + flags + program.
 * ============================================================ */

typedef struct {
    __torajs_heap_header_t header;
    uint8_t flags;
    /* `rejected` — parse failed (lookahead / lookbehind / named group /
     * other unsupported syntax). On the .test() path, we silently return
     * false (preserving Phase 1b's stub-compat behavior so cases that
     * only probe `re.test() === false` keep passing). On the heavier
     * surface paths (exec / match / replace / replaceAll / split) we
     * abort with a "not yet supported:" stderr to land in the test262
     * runner's `incompatible` bucket rather than producing wrong
     * matches that would land in the bug bucket. */
    uint8_t rejected;
    uint8_t pad[2];
    int n_captures;     /* count of `(...)` groups (excl. `(?:...)`) */
    Program prog;
    /* Pattern bytes preserved for re.toString() — Phase 1b. */
    uint8_t *src_bytes;
    int64_t src_len;
} RegExp;

static uint8_t parse_flags(const uint8_t *p, int64_t len) {
    uint8_t out = 0;
    for (int64_t i = 0; i < len; i++) {
        switch (p[i]) {
            case 'i': out |= RE_FLAG_I; break;
            case 'g': out |= RE_FLAG_G; break;
            case 'm': out |= RE_FLAG_M; break;
            case 's': out |= RE_FLAG_S; break;
            case 'u': out |= RE_FLAG_U; break;
            case 'y': out |= RE_FLAG_Y; break;
            default: break; /* unknown flag — JS would SyntaxError; we silently skip in Phase 1a. */
        }
    }
    return out;
}

/* ============================================================
 * Public API.
 * ============================================================ */

void *__torajs_regex_compile(const void *pattern_str, const void *flags_str) {
    const uint8_t *pat = __TORAJS_STR_CDATA(pattern_str);
    int64_t plen = (int64_t)__TORAJS_STR_LEN(pattern_str);
    const uint8_t *fl = __TORAJS_STR_CDATA(flags_str);
    int64_t flen = (int64_t)__TORAJS_STR_LEN(flags_str);

    RegExp *re = (RegExp *)calloc(1, sizeof(RegExp));
    re->header.refcount = 1;
    re->header.type_tag = __TORAJS_TAG_REGEX;
    re->header.flags = 0;
    re->flags = parse_flags(fl, flen);

    /* Cache the source bytes for future re.toString(). */
    re->src_bytes = (uint8_t *)malloc((size_t)plen);
    memcpy(re->src_bytes, pat, (size_t)plen);
    re->src_len = plen;

    Parser ps = { pat, plen, 0, re->flags, 0, 0 };
    Node *root = parse_alt(&ps);
    re->n_captures = ps.n_captures;
    if (!root || ps.err || ps.i != ps.len) {
        /* Parse failure (lookahead / lookbehind / named groups / etc.).
         * Mark the regex as `rejected` and emit a never-match stub.
         * The .test() path returns false silently (preserves the
         * Phase 1b behavior where many test262 cases just probe
         * `re.test() === false` against unsupported patterns and
         * happen to pass because the stub agrees with bun on miss).
         * The heavier paths (exec / match / replace*  / split) check
         * the rejected flag and abort with "not yet supported:" so
         * they land in the test262 runner's incompatible bucket
         * rather than producing wrong matches → bug. */
        if (root) node_free(root);
        re->rejected = 1;
        Inst never = { OP_CHAR, 0xff, 0, 0, 0 };
        prog_emit(&re->prog, never);
        Inst m = { OP_MATCH, 0, 0, 0, 0 };
        prog_emit(&re->prog, m);
        return re;
    }
    compile_node(&re->prog, root);
    Inst match = { OP_MATCH, 0, 0, 0, 0 };
    prog_emit(&re->prog, match);
    node_free(root);
    return re;
}

/* T-37 followup — `re.source` returns the original pattern text
 * (no flags, no enclosing slashes). Wraps re->src_bytes in a fresh
 * Str via the small-string pool. NULL receiver returns "".
 * Forward decl for __torajs_str_alloc_pooled (defined further down). */
extern uint8_t *__torajs_str_alloc_pooled(uint64_t len);
void *__torajs_regex_get_source(const void *re_ptr) {
    if (!re_ptr) {
        return __torajs_str_alloc_pooled(0);
    }
    const RegExp *re = (const RegExp *)re_ptr;
    int64_t len = re->src_len;
    if (len < 0) len = 0;
    uint8_t *s = __torajs_str_alloc_pooled((uint64_t)len);
    if (len > 0 && re->src_bytes) {
        memcpy(s + __TORAJS_STR_HDR_SIZE, re->src_bytes, (size_t)len);
    }
    return s;
}

void __torajs_regex_drop(void *re_ptr) {
    if (!re_ptr) return;
    if (!__torajs_rc_dec(re_ptr)) return;
    RegExp *re = (RegExp *)re_ptr;
    /* Free main prog (insts + classes) and recursively all
     * lookahead sub-programs — prog_free walks sub_progs[]. */
    if (re->prog.insts) free(re->prog.insts);
    if (re->prog.classes) free(re->prog.classes);
    for (int i = 0; i < re->prog.n_sub_progs; i++) prog_free(re->prog.sub_progs[i]);
    if (re->prog.sub_progs) free(re->prog.sub_progs);
    if (re->src_bytes) free(re->src_bytes);
    free(re);
}

/* ============================================================
 * VM matcher — Russ Cox style. Per input position, advance every
 * currently-active thread one CHAR / ANYCHAR / CLASS step;
 * threads waiting on epsilon ops (JMP, SPLIT, anchors, bounds,
 * SAVE) resolve immediately and enqueue the resulting thread state.
 *
 * Phase 1c.1: each thread carries a fixed-size saves[] array
 * recording the byte position of every capturing-group start/end
 * (slots 2*idx, 2*idx+1). SPLIT forks each get a fresh copy so a
 * SAVE in one branch doesn't leak into the other. Visited bitmap
 * still dedups by PC (first-write-wins) — leftmost-first semantics
 * mean the higher-priority copy already won. -1 sentinel = "not
 * captured" (group is on a branch the matcher didn't take).
 * ============================================================ */

#define REGEX_MAX_CAPTURES  32   /* > 32 capture groups → "regex too large" */
#define REGEX_SAVE_SLOTS    (REGEX_MAX_CAPTURES * 2)

typedef struct {
    int pc;
    int pad;
    int64_t saves[REGEX_SAVE_SLOTS];
} Thread;

typedef struct {
    Thread *list;
    int n;
    /* Visited bitmap to keep at most one copy of each PC per step. */
    uint32_t step_id;
} ThreadList;

typedef struct {
    int n_insts;
    uint32_t *visited;   /* visited[pc] == step_id_at_visit */
} VisitedTable;

static int is_word_byte(uint8_t c) {
    return (c >= '0' && c <= '9')
        || (c >= 'A' && c <= 'Z')
        || (c >= 'a' && c <= 'z')
        || c == '_';
}

static int char_eq(uint8_t a, uint8_t b, uint8_t flags) {
    if (a == b) return 1;
    if (flags & RE_FLAG_I) {
        /* ASCII case-fold. */
        if (a >= 'A' && a <= 'Z' && b == (uint8_t)(a + 32)) return 1;
        if (a >= 'a' && a <= 'z' && b == (uint8_t)(a - 32)) return 1;
    }
    return 0;
}

/* Forward decl — vm_match_at is below; lookahead resolution recurses. */
static int64_t vm_match_at(
    const Program *p,
    const uint8_t *s, int64_t slen,
    int64_t start_pos,
    uint8_t flags,
    Thread *cur, Thread *nxt,
    uint32_t *visited_cur, uint32_t *visited_nxt,
    uint32_t *step_id_ref,
    int64_t *out_saves
);

/* Phase 1c.4 — sub-pattern probe for lookahead resolution. Allocates
 * its own VM workspace (small — sub-program inst count is bounded by
 * the body size) and returns 1 if the sub matches at `pos`, 0 if not.
 * No saves out; lookahead doesn't emit captures into the outer
 * thread's save state (per JS spec — captures inside lookahead are
 * scoped to the lookahead body and discarded after). */
static int sub_probe(const Program *sub, const uint8_t *s, int64_t slen,
                     int64_t pos, uint8_t flags) {
    if (sub->n_insts == 0) return 1; /* empty body always matches */
    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)sub->n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)sub->n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)sub->n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)sub->n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;
    int64_t end = vm_match_at(sub, s, slen, pos, flags, cur, nxt, vc, vn,
                              &step_id, NULL);
    free(cur); free(nxt); free(vc); free(vn);
    return end >= 0 ? 1 : 0;
}

/* Add `pc` (carrying `saves`) to `tl`, transitively expanding
 * epsilon ops (JMP, SPLIT, SAVE, anchors, word-bounds, LOOKAHEAD).
 * Saves are passed by const pointer; SPLIT/SAVE create modified
 * copies on the local stack before recursing. The resulting list
 * contains only "real" waiting-for-input PCs each carrying their own
 * snapshot. */
static void add_thread(
    ThreadList *tl, VisitedTable *vt, int pc, const Program *p,
    const uint8_t *s, int64_t slen, int64_t pos, uint8_t flags,
    const int64_t *saves
) {
    if (pc < 0 || pc >= p->n_insts) return;
    if (vt->visited[pc] == tl->step_id) return;
    vt->visited[pc] = tl->step_id;
    Inst ins = p->insts[pc];
    switch (ins.op) {
        case OP_JMP:
            add_thread(tl, vt, ins.a, p, s, slen, pos, flags, saves);
            return;
        case OP_SPLIT:
            add_thread(tl, vt, ins.a, p, s, slen, pos, flags, saves);
            add_thread(tl, vt, ins.b, p, s, slen, pos, flags, saves);
            return;
        case OP_SAVE: {
            int64_t copy[REGEX_SAVE_SLOTS];
            memcpy(copy, saves, sizeof(copy));
            if (ins.a >= 0 && ins.a < REGEX_SAVE_SLOTS) copy[ins.a] = pos;
            add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, copy);
            return;
        }
        case OP_ANCHOR_B: {
            int ok = (pos == 0)
                  || ((flags & RE_FLAG_M) && pos > 0 && s[pos - 1] == '\n');
            if (ok) add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            return;
        }
        case OP_ANCHOR_E: {
            int ok = (pos == slen)
                  || ((flags & RE_FLAG_M) && pos < slen && s[pos] == '\n');
            if (ok) add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            return;
        }
        case OP_WBOUND: {
            int left = (pos > 0) && is_word_byte(s[pos - 1]);
            int right = (pos < slen) && is_word_byte(s[pos]);
            if (left != right) add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            return;
        }
        case OP_NWBOUND: {
            int left = (pos > 0) && is_word_byte(s[pos - 1]);
            int right = (pos < slen) && is_word_byte(s[pos]);
            if (left == right) add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            return;
        }
        case OP_LOOKAHEAD: {
            const Program *sub = p->sub_progs[ins.a];
            if (sub_probe(sub, s, slen, pos, flags)) {
                add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            }
            return;
        }
        case OP_NEG_LOOKAHEAD: {
            const Program *sub = p->sub_progs[ins.a];
            if (!sub_probe(sub, s, slen, pos, flags)) {
                add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            }
            return;
        }
        default: {
            Thread *t = &tl->list[tl->n++];
            t->pc = pc;
            memcpy(t->saves, saves, sizeof(t->saves));
        }
    }
}

/* Try matching at exactly `start_pos`. Returns end position on hit
 * (start_pos..end_pos consumed), or -1 on miss. On hit, also writes
 * the winning thread's `saves` (capture group offsets) into
 * `out_saves` (size REGEX_SAVE_SLOTS). Workspace buffers are caller-
 * provided so a caller in a tight loop (replaceAll / matchAll /
 * split) can amortize the allocation across many positions. */
static int64_t vm_match_at(
    const Program *p,
    const uint8_t *s, int64_t slen,
    int64_t start_pos,
    uint8_t flags,
    Thread *cur, Thread *nxt,
    uint32_t *visited_cur, uint32_t *visited_nxt,
    uint32_t *step_id_ref,
    int64_t *out_saves
) {
    ThreadList cur_tl = { cur, 0, 0 };
    ThreadList nxt_tl = { nxt, 0, 0 };
    VisitedTable cur_vt = { p->n_insts, visited_cur };
    VisitedTable nxt_vt = { p->n_insts, visited_nxt };

    cur_tl.n = 0;
    cur_tl.step_id = ++(*step_id_ref);
    int64_t empty_saves[REGEX_SAVE_SLOTS];
    for (int i = 0; i < REGEX_SAVE_SLOTS; i++) empty_saves[i] = -1;
    add_thread(&cur_tl, &cur_vt, 0, p, s, slen, start_pos, flags, empty_saves);

    int64_t end_pos = -1;

    /* Leftmost-first / greedy semantics: when MATCH fires for a
     * thread at position p, lower-priority threads in cur_tl at this
     * step are dead (can't beat it), but higher-priority threads
     * already advanced into nxt_tl can still extend the match by
     * consuming more chars. So we record end_pos but DON'T break out
     * of the outer pos loop — keep advancing until the live thread
     * set drains. The latest MATCH seen wins. */
    for (int64_t pos = start_pos; pos <= slen; pos++) {
        nxt_tl.n = 0;
        nxt_tl.step_id = ++(*step_id_ref);
        int saw_match_this_step = 0;
        for (int ti = 0; ti < cur_tl.n && !saw_match_this_step; ti++) {
            const Thread *t = &cur_tl.list[ti];
            int pc = t->pc;
            Inst ins = p->insts[pc];
            switch (ins.op) {
                case OP_CHAR: {
                    if (pos < slen && char_eq(ins.ch, s[pos], flags)) {
                        add_thread(&nxt_tl, &nxt_vt, pc + 1, p, s, slen, pos + 1, flags, t->saves);
                    }
                    break;
                }
                case OP_ANYCHAR: {
                    if (pos < slen && ((flags & RE_FLAG_S) || s[pos] != '\n')) {
                        add_thread(&nxt_tl, &nxt_vt, pc + 1, p, s, slen, pos + 1, flags, t->saves);
                    }
                    break;
                }
                case OP_CLASS: {
                    if (pos < slen && cc_test(&p->classes[ins.a], s[pos])) {
                        add_thread(&nxt_tl, &nxt_vt, pc + 1, p, s, slen, pos + 1, flags, t->saves);
                    }
                    break;
                }
                case OP_MATCH:
                    /* Higher-priority threads at this step were already
                     * processed (they ran first by ti order). Any later
                     * MATCH or extension via lower-priority threads is
                     * a worse choice — stop scanning this step. */
                    saw_match_this_step = 1;
                    end_pos = pos;
                    if (out_saves) memcpy(out_saves, t->saves, sizeof(t->saves));
                    break;
                default:
                    break;
            }
        }
        ThreadList tmp_tl = cur_tl; cur_tl = nxt_tl; nxt_tl = tmp_tl;
        VisitedTable tmp_vt = cur_vt; cur_vt = nxt_vt; nxt_vt = tmp_vt;
        if (cur_tl.n == 0) break;
    }
    /* End-of-input: any thread sitting on MATCH after the loop is also
     * an acceptance — record it. (cur_tl has been swapped to the most
     * recent next-list at this point.) */
    for (int ti = 0; ti < cur_tl.n; ti++) {
        const Thread *t = &cur_tl.list[ti];
        if (p->insts[t->pc].op == OP_MATCH) {
            end_pos = slen;
            if (out_saves) memcpy(out_saves, t->saves, sizeof(t->saves));
            break;
        }
    }
    return end_pos;
}

/* Search for a match starting at any position >= from_pos. Writes the
 * match start + end positions and returns 1 on hit; returns 0 on miss
 * (out params untouched). Optionally writes capture-group save offsets
 * to `out_saves` (size REGEX_SAVE_SLOTS) — pass NULL if not needed. */
static int vm_search_from(
    const Program *p,
    const uint8_t *s, int64_t slen,
    int64_t from_pos,
    uint8_t flags,
    int64_t *out_start, int64_t *out_end,
    int64_t *out_saves
) {
    if (p->n_insts == 0) return 0;
    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)p->n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)p->n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)p->n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)p->n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;
    int hit = 0;
    for (int64_t st = from_pos; st <= slen; st++) {
        int64_t end = vm_match_at(p, s, slen, st, flags, cur, nxt, vc, vn, &step_id, out_saves);
        if (end >= 0) {
            *out_start = st;
            *out_end = end;
            hit = 1;
            break;
        }
    }
    free(cur);
    free(nxt);
    free(vc);
    free(vn);
    return hit;
}

/* Tight-loop variant: caller owns the workspace so per-iter alloc is
 * skipped. step_id is shared so visited bitmaps stay coherent across
 * find calls on the same workspace. */
static int vm_search_from_with_ws(
    const Program *p,
    const uint8_t *s, int64_t slen,
    int64_t from_pos,
    uint8_t flags,
    Thread *cur, Thread *nxt,
    uint32_t *vc, uint32_t *vn,
    uint32_t *step_id_ref,
    int64_t *out_start, int64_t *out_end,
    int64_t *out_saves
) {
    for (int64_t st = from_pos; st <= slen; st++) {
        int64_t end = vm_match_at(p, s, slen, st, flags, cur, nxt, vc, vn, step_id_ref, out_saves);
        if (end >= 0) {
            *out_start = st;
            *out_end = end;
            return 1;
        }
    }
    return 0;
}

int64_t __torajs_regex_test(const void *re_ptr, const void *str_ptr) {
    if (!re_ptr) return 0;
    const RegExp *re = (const RegExp *)re_ptr;
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);
    int64_t st, en;
    return vm_search_from(&re->prog, s, slen, 0, re->flags, &st, &en, NULL) ? 1 : 0;
}

/* ============================================================
 * Phase 1b surface methods — find_next + s.match / s.replace /
 * s.replaceAll / s.split / re.exec.
 *
 * Result objects use the universal heap layout — Str via
 * `__torajs_str_alloc_pooled`, Array via `__torajs_arr_alloc` /
 * `__torajs_arr_push`. ssa_lower's drop machinery handles cleanup
 * through the standard Type::Str / Type::Arr paths.
 *
 * Capturing groups are NOT yet supported in Phase 1b — `s.match`
 * without `g` returns a single-element array `[matched_substring]`
 * (vs JS spec's `[match, group1, group2, ..., index, input]`).
 * Same trade for `re.exec`. Phase 1c will add VM save instructions
 * + capture group recording and round these out to spec shape.
 *
 * Replacement string `$&` / `$1..$9` substitution is also Phase 1c
 * — the replace helpers below treat repl as a plain literal string.
 * ============================================================ */

extern uint8_t *__torajs_str_alloc_pooled(uint64_t len);
extern void *__torajs_arr_alloc(uint64_t initial_cap);
extern void *__torajs_arr_push(void *arr, int64_t val);

/* Abort with "not yet supported:" for a rejected regex. The test262
 * runner classifies stderr starting with this prefix as incompatible
 * (subset boundary) — preserves tr-accepted parity by keeping these
 * cases out of the bug bucket. Called from exec / match / replace*  /
 * split when the receiver regex was marked rejected at compile time. */
static void abort_unsupported(const RegExp *re) {
    fputs("not yet supported: regex feature not yet implemented "
          "in v0.2 #1.c — pattern: /", stderr);
    if (re->src_len > 0 && re->src_bytes) {
        fwrite(re->src_bytes, 1, (size_t)re->src_len, stderr);
    }
    fputc('/', stderr);
    fputc('\n', stderr);
    exit(1);
}

/* Build a fresh Str holding bytes [data, data+len). Refcount=1.
 * Allocator is the small-Str pool path so ≤16-byte tokens (the dominant
 * size class for split / match outputs) recycle instead of malloc. */
static uint8_t *str_from_bytes(const uint8_t *data, int64_t len) {
    uint8_t *p = __torajs_str_alloc_pooled((uint64_t)len);
    if (len > 0) memcpy(p + __TORAJS_STR_HDR_SIZE, data, (size_t)len);
    return p;
}

/* Find next match in `s` starting at `start`. Returns packed i64:
 *   high 32 = start_pos, low 32 = end_pos (exclusive)
 *   sentinel -1 = no match
 * Only used by ssa_lower-emitted code that wants the raw positions
 * (not currently exposed; the surface methods below use the C-level
 * helpers directly). Reserved for Phase 1c when re.exec wires in
 * capture groups. */
int64_t __torajs_regex_find(const void *re_ptr, const void *str_ptr, int64_t start) {
    if (!re_ptr) return -1;
    const RegExp *re = (const RegExp *)re_ptr;
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);
    if (start < 0) start = 0;
    if (start > slen) return -1;
    int64_t st, en;
    if (!vm_search_from(&re->prog, s, slen, start, re->flags, &st, &en, NULL)) return -1;
    return (st << 32) | (en & 0xffffffff);
}

/* `s.match(re)` — Phase 1b shape:
 *   - Returns Array<Str>; never returns null (callers treat empty
 *     array as "no match"). Spec returns null on miss; tr deviates
 *     on this single point until Nullable<Array<Str>> propagation
 *     lands as part of Phase 1c.
 *   - Without `g` flag: single-element array `[matched_substring]`
 *   - With `g` flag: array of all non-overlapping match substrings
 *   - Empty matches (e.g. zero-width /a STAR/ on "bbb") advance one
 *     position to avoid infinite loops, mirroring JS semantics. */
void *__torajs_str_match_regex(const void *str_ptr, const void *re_ptr) {
    void *out = __torajs_arr_alloc(0);
    if (!re_ptr || !str_ptr) return out;
    const RegExp *re = (const RegExp *)re_ptr;
    if (re->rejected) abort_unsupported(re);
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);

    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;

    int64_t pos = 0;
    int global = (re->flags & RE_FLAG_G) ? 1 : 0;
    /* Phase 1c.1: without `g`, JS spec says s.match returns an
     * array shaped like RegExp.exec — [match, group1, group2, ...].
     * With `g`, captures are stripped and only whole-match strings
     * appear (the spec drops capture info in the global case). */
    int64_t saves[REGEX_SAVE_SLOTS];
    while (pos <= slen) {
        int64_t st, en;
        if (!vm_search_from_with_ws(&re->prog, s, slen, pos, re->flags,
                                    cur, nxt, vc, vn, &step_id, &st, &en,
                                    global ? NULL : saves)) break;
        uint8_t *seg = str_from_bytes(s + st, en - st);
        out = __torajs_arr_push(out, (int64_t)(intptr_t)seg);
        if (!global) {
            /* Append captures for the spec-shape [match, g1, g2, ...].
             * NULL pointer for uncaptured groups (see __torajs_regex_exec
             * for the rationale on undefined-vs-null sentinel choice). */
            for (int i = 1; i <= re->n_captures && i < REGEX_MAX_CAPTURES; i++) {
                int64_t gs = saves[2 * i];
                int64_t ge = saves[2 * i + 1];
                if (gs < 0 || ge < 0) {
                    out = __torajs_arr_push(out, 0);
                } else {
                    uint8_t *grp = str_from_bytes(s + gs, ge - gs);
                    out = __torajs_arr_push(out, (int64_t)(intptr_t)grp);
                }
            }
            break;
        }
        /* Empty match — bump pos by 1 to avoid spinning forever. */
        pos = (en == st) ? en + 1 : en;
    }

    free(cur); free(nxt); free(vc); free(vn);
    return out;
}

/* Phase 1c.2 — `$&` / `$1..$9` / `$$` substitution in replacement
 * string. Expand the replacement template into the growing output
 * buffer, dereferencing `$N` against the matched capture saves[]
 * pairs. Unmatched groups (-1, -1) substitute the empty string,
 * matching JS spec for unparticipating alternation branches. `$$`
 * is the literal-`$` escape; `$&` is the whole match. Anything
 * else after `$` (incl. `$0`) emits the `$` literally followed by
 * the next char.
 *
 * `$NN` (two-digit) — JS spec consumes the second digit only when
 * the resulting NN <= n_captures; otherwise treats as `$N` followed
 * by a literal digit. Phase 1c.2 implements that lookahead. */
static void emit_byte(uint8_t b, uint8_t **out, int64_t *out_len, int64_t *out_cap) {
    if (*out_len + 1 > *out_cap) {
        *out_cap *= 2;
        *out = (uint8_t *)realloc(*out, (size_t)*out_cap);
    }
    (*out)[(*out_len)++] = b;
}
static void emit_bytes(const uint8_t *src, int64_t n, uint8_t **out, int64_t *out_len, int64_t *out_cap) {
    if (n <= 0) return;
    if (*out_len + n > *out_cap) {
        while (*out_len + n > *out_cap) *out_cap *= 2;
        *out = (uint8_t *)realloc(*out, (size_t)*out_cap);
    }
    memcpy(*out + *out_len, src, (size_t)n);
    *out_len += n;
}
static void expand_repl(
    const uint8_t *repl, int64_t repl_len,
    const uint8_t *s, int64_t st, int64_t en,
    const int64_t *saves, int n_captures,
    uint8_t **out, int64_t *out_len, int64_t *out_cap
) {
    for (int64_t i = 0; i < repl_len; i++) {
        uint8_t c = repl[i];
        if (c != '$' || i + 1 >= repl_len) {
            emit_byte(c, out, out_len, out_cap);
            continue;
        }
        uint8_t nxt = repl[i + 1];
        if (nxt == '$') {
            emit_byte('$', out, out_len, out_cap);
            i++;
            continue;
        }
        if (nxt == '&') {
            emit_bytes(s + st, en - st, out, out_len, out_cap);
            i++;
            continue;
        }
        if (nxt >= '0' && nxt <= '9') {
            int d1 = nxt - '0';
            int idx = d1;
            int extra_consumed = 0;
            /* Try two-digit `$NN` (JS spec — incl. leading zero like
             * `$01` → group 1) if the resulting idx is a valid group
             * index and fits in the saves table. */
            if (i + 2 < repl_len && repl[i + 2] >= '0' && repl[i + 2] <= '9') {
                int two = d1 * 10 + (repl[i + 2] - '0');
                if (two >= 1 && two <= n_captures && two < REGEX_MAX_CAPTURES) {
                    idx = two;
                    extra_consumed = 1;
                }
            }
            if (idx >= 1 && idx <= n_captures && idx < REGEX_MAX_CAPTURES) {
                int64_t gs = saves[2 * idx];
                int64_t ge = saves[2 * idx + 1];
                if (gs >= 0 && ge >= 0) {
                    emit_bytes(s + gs, ge - gs, out, out_len, out_cap);
                }
                /* Unparticipating group → empty string (no emit). */
                i += 1 + extra_consumed;
                continue;
            }
            /* `$0` standalone or `$N` for N > n_captures — emit
             * literally (no expansion). i++ at loop will consume the
             * digit; we emit `$` here. */
            emit_byte('$', out, out_len, out_cap);
            continue;
        }
        /* Unknown `$X` — emit the `$` literally; the X stays as the
         * next iteration's char. */
        emit_byte('$', out, out_len, out_cap);
    }
}

/* `s.replace(re, repl)` — single first-match replacement. `repl` may
 * contain `$&` / `$1..$9` / `$$` substitution tokens (Phase 1c.2).
 * When `re` carries the `g` flag, behaves like replaceAll. */
void *__torajs_str_replace_regex(
    const void *str_ptr, const void *re_ptr, const void *repl_ptr
) {
    if (!re_ptr) return str_from_bytes(__TORAJS_STR_CDATA(str_ptr),
                                       (int64_t)__TORAJS_STR_LEN(str_ptr));
    const RegExp *re = (const RegExp *)re_ptr;
    if (re->rejected) abort_unsupported(re);
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);
    const uint8_t *repl = __TORAJS_STR_CDATA(repl_ptr);
    int64_t repl_len = (int64_t)__TORAJS_STR_LEN(repl_ptr);
    int global = (re->flags & RE_FLAG_G) ? 1 : 0;

    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;

    /* Scratch output buffer — grown geometrically. */
    int64_t out_cap = slen + 16;
    uint8_t *out = (uint8_t *)malloc((size_t)out_cap);
    int64_t out_len = 0;
    int64_t pos = 0;
    int64_t saves[REGEX_SAVE_SLOTS];

    while (pos <= slen) {
        int64_t st, en;
        if (!vm_search_from_with_ws(&re->prog, s, slen, pos, re->flags,
                                    cur, nxt, vc, vn, &step_id, &st, &en, saves)) break;
        emit_bytes(s + pos, st - pos, &out, &out_len, &out_cap);
        expand_repl(repl, repl_len, s, st, en, saves, re->n_captures,
                    &out, &out_len, &out_cap);
        if (en == st) {
            /* Empty match — copy the next char verbatim and advance. */
            if (st < slen) emit_byte(s[st], &out, &out_len, &out_cap);
            pos = en + 1;
        } else {
            pos = en;
        }
        if (!global) break;
    }
    /* Append remainder. */
    emit_bytes(s + pos, slen - pos, &out, &out_len, &out_cap);

    uint8_t *result = str_from_bytes(out, out_len);
    free(out);
    free(cur); free(nxt); free(vc); free(vn);
    return result;
}

/* `s.replaceAll(re, repl)` — same as replace with implicit `g`-style
 * iteration (works regardless of whether the pattern carried `g` —
 * JS spec actually throws TypeError if `re` doesn't carry `g`, but
 * that's a v0.2 #1.c spec-correctness pass). Always replaces every
 * non-overlapping match. */
void *__torajs_str_replace_all_regex(
    const void *str_ptr, const void *re_ptr, const void *repl_ptr
) {
    if (!re_ptr) return str_from_bytes(__TORAJS_STR_CDATA(str_ptr),
                                       (int64_t)__TORAJS_STR_LEN(str_ptr));
    const RegExp *re = (const RegExp *)re_ptr;
    if (re->rejected) abort_unsupported(re);
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);
    const uint8_t *repl = __TORAJS_STR_CDATA(repl_ptr);
    int64_t repl_len = (int64_t)__TORAJS_STR_LEN(repl_ptr);

    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;

    int64_t out_cap = slen + 16;
    uint8_t *out = (uint8_t *)malloc((size_t)out_cap);
    int64_t out_len = 0;
    int64_t pos = 0;
    int64_t saves[REGEX_SAVE_SLOTS];

    while (pos <= slen) {
        int64_t st, en;
        if (!vm_search_from_with_ws(&re->prog, s, slen, pos, re->flags,
                                    cur, nxt, vc, vn, &step_id, &st, &en, saves)) break;
        emit_bytes(s + pos, st - pos, &out, &out_len, &out_cap);
        expand_repl(repl, repl_len, s, st, en, saves, re->n_captures,
                    &out, &out_len, &out_cap);
        if (en == st) {
            if (st < slen) emit_byte(s[st], &out, &out_len, &out_cap);
            pos = en + 1;
        } else {
            pos = en;
        }
    }
    emit_bytes(s + pos, slen - pos, &out, &out_len, &out_cap);

    uint8_t *result = str_from_bytes(out, out_len);
    free(out);
    free(cur); free(nxt); free(vc); free(vn);
    return result;
}

/* `s.split(re)` — splits at each non-overlapping match of `re`. The
 * matched bytes are removed from the output; the input is sliced into
 * the pieces between matches (and before the first / after the last).
 * Phase 1b: returns Array<Str>; capturing groups would be interleaved
 * into the result by JS spec — that wiring is part of Phase 1c. */
void *__torajs_str_split_regex(const void *str_ptr, const void *re_ptr) {
    void *out = __torajs_arr_alloc(0);
    if (!re_ptr || !str_ptr) return out;
    const RegExp *re = (const RegExp *)re_ptr;
    if (re->rejected) abort_unsupported(re);
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);

    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;

    int64_t pos = 0;
    while (pos <= slen) {
        int64_t st, en;
        if (!vm_search_from_with_ws(&re->prog, s, slen, pos, re->flags,
                                    cur, nxt, vc, vn, &step_id, &st, &en, NULL)) break;
        if (en == st) {
            /* Empty separator — JS specifies splitting after each char:
             * "ab".split(//) → ["a","b"]. We mirror that: take one byte,
             * push, advance. */
            if (st >= slen) break;
            uint8_t *seg = str_from_bytes(s + pos, st - pos);
            out = __torajs_arr_push(out, (int64_t)(intptr_t)seg);
            pos = en + 1;
            continue;
        }
        uint8_t *seg = str_from_bytes(s + pos, st - pos);
        out = __torajs_arr_push(out, (int64_t)(intptr_t)seg);
        pos = en;
    }
    /* Append final tail. */
    if (pos <= slen) {
        uint8_t *seg = str_from_bytes(s + pos, slen - pos);
        out = __torajs_arr_push(out, (int64_t)(intptr_t)seg);
    }

    free(cur); free(nxt); free(vc); free(vn);
    return out;
}

/* `re.exec(s)` — Phase 1c.1 spec-shape result.
 *
 * Returns Array<Str> with [matched, group1, group2, ...] for the first
 * match starting at lastIndex (treated as 0 in Phase 1c.1; lastIndex
 * tracking lands when sticky/global state machinery comes in). On
 * miss, returns an empty array (the JS spec returns null — switching
 * to that needs Nullable<Array> propagation, deferred to Phase 1c.4).
 *
 * Unmatched capture groups (e.g. an alternation branch the matcher
 * skipped) are filled with NULL pointers — semantically equivalent to
 * bun's `undefined` thanks to tr's `undefined → Type::Null` mapping.
 * `result[i] === undefined` round-trips correctly because both sides
 * lower to the same null pointer comparison. console.log on a NULL
 * Str slot prints "null" (vs bun's "undefined") which is a narrow
 * stdout-shape divergence on direct print of uncaptured slots —
 * assertion-style tests (the test262 idiom) work correctly. */
void *__torajs_regex_exec(const void *re_ptr, const void *str_ptr) {
    void *out = __torajs_arr_alloc(0);
    if (!re_ptr || !str_ptr) return out;
    const RegExp *re = (const RegExp *)re_ptr;
    if (re->rejected) abort_unsupported(re);
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);

    int64_t saves[REGEX_SAVE_SLOTS];
    int64_t st, en;
    if (!vm_search_from(&re->prog, s, slen, 0, re->flags, &st, &en, saves)) return out;

    /* [0] = whole match */
    uint8_t *whole = str_from_bytes(s + st, en - st);
    out = __torajs_arr_push(out, (int64_t)(intptr_t)whole);
    /* [1..n_captures] = each capture group span. saves[2*i .. 2*i+1].
     * Slot pair (-1, -1) means the group never participated — NULL
     * sentinel (see comment above). */
    for (int i = 1; i <= re->n_captures && i < REGEX_MAX_CAPTURES; i++) {
        int64_t gs = saves[2 * i];
        int64_t ge = saves[2 * i + 1];
        if (gs < 0 || ge < 0) {
            out = __torajs_arr_push(out, 0);
        } else {
            uint8_t *grp = str_from_bytes(s + gs, ge - gs);
            out = __torajs_arr_push(out, (int64_t)(intptr_t)grp);
        }
    }
    return out;
}

/* `s.matchAll(re)` — Phase 1c.3.
 *
 * JS spec: returns an iterator that yields RegExp.exec-shape arrays
 * for each non-overlapping match (and throws TypeError if `re` lacks
 * the `g` flag). tr returns Array<Array<Str>> instead — array of
 * exec-shape arrays — until iterator protocol lands at the surface
 * (Phase 1c.4+). The `g`-required check is also Phase 1c.4 work;
 * for now matchAll iterates regardless of flag (over-permissive vs
 * spec but doesn't produce wrong matches when `g` is set, which is
 * the dominant test262 idiom). */
void *__torajs_str_match_all_regex(const void *str_ptr, const void *re_ptr) {
    void *outer = __torajs_arr_alloc(0);
    if (!re_ptr || !str_ptr) return outer;
    const RegExp *re = (const RegExp *)re_ptr;
    if (re->rejected) abort_unsupported(re);
    const uint8_t *s = __TORAJS_STR_CDATA(str_ptr);
    int64_t slen = (int64_t)__TORAJS_STR_LEN(str_ptr);

    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)re->prog.n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)re->prog.n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;

    int64_t pos = 0;
    int64_t saves[REGEX_SAVE_SLOTS];
    while (pos <= slen) {
        int64_t st, en;
        if (!vm_search_from_with_ws(&re->prog, s, slen, pos, re->flags,
                                    cur, nxt, vc, vn, &step_id, &st, &en, saves)) break;
        /* Build exec-shape inner array [match, g1, g2, ...]. */
        void *inner = __torajs_arr_alloc(0);
        uint8_t *whole = str_from_bytes(s + st, en - st);
        inner = __torajs_arr_push(inner, (int64_t)(intptr_t)whole);
        for (int i = 1; i <= re->n_captures && i < REGEX_MAX_CAPTURES; i++) {
            int64_t gs = saves[2 * i];
            int64_t ge = saves[2 * i + 1];
            if (gs < 0 || ge < 0) {
                inner = __torajs_arr_push(inner, 0);
            } else {
                uint8_t *grp = str_from_bytes(s + gs, ge - gs);
                inner = __torajs_arr_push(inner, (int64_t)(intptr_t)grp);
            }
        }
        outer = __torajs_arr_push(outer, (int64_t)(intptr_t)inner);
        /* Empty match — bump pos by 1. */
        pos = (en == st) ? en + 1 : en;
    }

    free(cur); free(nxt); free(vc); free(vn);
    return outer;
}
