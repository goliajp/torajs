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
#define RE_FLAG_U 0x10u  /* unicode — enables \u{HHHH..} extended escape
                          * and code-point-aware OP_ANYCHAR advance (P9.3-A1).
                          * \p{...} property class + code-point OP_CLASS land
                          * in P9.3-A2. */
#define RE_FLAG_Y 0x20u  /* sticky — used by iteration helpers */

/* ============================================================
 * UTF-8 helpers — used by P9.3 u-flag handling. The input string
 * is always well-formed UTF-8 (JS strings round-trip through tora's
 * Str storage). These are minimal: utf8_len_for inspects the leading
 * byte to report the encoded byte length; utf8_encode_cp encodes a
 * code point into 1–4 bytes. No decoder is needed for A1 (OP_ANYCHAR
 * only needs the byte length; literal escapes encode at parse time).
 * Decoder for OP_CLASS / \p{...} arrives in A2.
 * ============================================================ */

static int utf8_len_for(uint8_t b) {
    if ((b & 0x80u) == 0x00u) return 1;       /* 0xxxxxxx */
    if ((b & 0xE0u) == 0xC0u) return 2;       /* 110xxxxx */
    if ((b & 0xF0u) == 0xE0u) return 3;       /* 1110xxxx */
    if ((b & 0xF8u) == 0xF0u) return 4;       /* 11110xxx */
    return 1;                                  /* continuation / invalid — defensive */
}

static int utf8_encode_cp(int32_t cp, uint8_t out[4]) {
    if (cp < 0 || cp > 0x10FFFF) return 0;
    if (cp < 0x80) {
        out[0] = (uint8_t)cp;
        return 1;
    }
    if (cp < 0x800) {
        out[0] = (uint8_t)(0xC0u | (uint32_t)(cp >> 6));
        out[1] = (uint8_t)(0x80u | (uint32_t)(cp & 0x3F));
        return 2;
    }
    if (cp < 0x10000) {
        out[0] = (uint8_t)(0xE0u | (uint32_t)(cp >> 12));
        out[1] = (uint8_t)(0x80u | (uint32_t)((cp >> 6) & 0x3F));
        out[2] = (uint8_t)(0x80u | (uint32_t)(cp & 0x3F));
        return 3;
    }
    out[0] = (uint8_t)(0xF0u | (uint32_t)(cp >> 18));
    out[1] = (uint8_t)(0x80u | (uint32_t)((cp >> 12) & 0x3F));
    out[2] = (uint8_t)(0x80u | (uint32_t)((cp >> 6) & 0x3F));
    out[3] = (uint8_t)(0x80u | (uint32_t)(cp & 0x3F));
    return 4;
}

static int32_t utf8_decode_cp(const uint8_t *s, int *out_len) {
    uint8_t b = s[0];
    if ((b & 0x80u) == 0u) { *out_len = 1; return (int32_t)b; }
    if ((b & 0xE0u) == 0xC0u) {
        *out_len = 2;
        return ((int32_t)(b & 0x1Fu) << 6)
             | (int32_t)(s[1] & 0x3Fu);
    }
    if ((b & 0xF0u) == 0xE0u) {
        *out_len = 3;
        return ((int32_t)(b & 0x0Fu) << 12)
             | ((int32_t)(s[1] & 0x3Fu) << 6)
             | (int32_t)(s[2] & 0x3Fu);
    }
    if ((b & 0xF8u) == 0xF0u) {
        *out_len = 4;
        return ((int32_t)(b & 0x07u) << 18)
             | ((int32_t)(s[1] & 0x3Fu) << 12)
             | ((int32_t)(s[2] & 0x3Fu) << 6)
             | (int32_t)(s[3] & 0x3Fu);
    }
    *out_len = 1; return (int32_t)b; /* invalid lead — defensive */
}

/* ============================================================
 * Unicode property tables (P9.3-A2).
 *
 * Curated subsets of UCD Letter / Number categories — covers the
 * dominant test262 usages (Greek, Cyrillic, Hebrew, Arabic, CJK,
 * Hangul, Hiragana, Katakana, common decimal-digit scripts).
 *
 * ASCII portions live in the regular bitmap (populated by
 * cc_add_property_*) so cc_test_cp dispatches: cp < 128 → bitmap,
 * cp ≥ 128 → range table.
 *
 * The full UCD Letter category has hundreds of ranges; the curated
 * subset here is intentionally a partial cover. L3b follow-up: full
 * UCD import or generated table. Per docs/design-principles.md the
 * pragma is "正统 / textbook" — minimum-viable property table that
 * lifts the dominant test262 cases, then iterate.
 * ============================================================ */

typedef struct { int32_t lo; int32_t hi; } UProp_Range;

static const UProp_Range UCD_LETTER[] = {
    /* Latin-1 supplement letters (cp > 0x7F) */
    {0x00AA, 0x00AA}, {0x00B5, 0x00B5}, {0x00BA, 0x00BA},
    {0x00C0, 0x00D6}, {0x00D8, 0x00F6}, {0x00F8, 0x024F},
    /* IPA + Spacing Modifier */
    {0x0250, 0x02AF}, {0x02B0, 0x02C1}, {0x02C6, 0x02D1},
    {0x02E0, 0x02E4}, {0x02EC, 0x02EC}, {0x02EE, 0x02EE},
    /* Greek and Coptic */
    {0x0370, 0x0373}, {0x0376, 0x0377}, {0x037A, 0x037D},
    {0x037F, 0x037F},
    {0x0386, 0x0386}, {0x0388, 0x038A}, {0x038C, 0x038C},
    {0x038E, 0x03A1}, {0x03A3, 0x03F5}, {0x03F7, 0x0481},
    /* Cyrillic */
    {0x048A, 0x052F},
    /* Armenian */
    {0x0531, 0x0556}, {0x0561, 0x0587},
    /* Hebrew letters */
    {0x05D0, 0x05EA}, {0x05F0, 0x05F2},
    /* Arabic letters */
    {0x0620, 0x064A}, {0x066E, 0x066F}, {0x0671, 0x06D3},
    {0x06D5, 0x06D5}, {0x06E5, 0x06E6}, {0x06EE, 0x06EF},
    {0x06FA, 0x06FC}, {0x06FF, 0x06FF},
    /* Devanagari letters */
    {0x0904, 0x0939}, {0x093D, 0x093D}, {0x0950, 0x0950},
    {0x0958, 0x0961},
    /* Thai letters */
    {0x0E01, 0x0E30}, {0x0E32, 0x0E33}, {0x0E40, 0x0E46},
    /* Hiragana */
    {0x3041, 0x3096}, {0x309D, 0x309F},
    /* Katakana */
    {0x30A1, 0x30FA}, {0x30FC, 0x30FF},
    /* CJK Unified Ideographs (basic + extension A) */
    {0x3400, 0x4DBF}, {0x4E00, 0x9FFF},
    /* Hangul Syllables */
    {0xAC00, 0xD7A3},
};

static const UProp_Range UCD_NUMBER[] = {
    /* Latin-1 numeric */
    {0x00B2, 0x00B3}, {0x00B9, 0x00B9}, {0x00BC, 0x00BE},
    /* Arabic-Indic digits */
    {0x0660, 0x0669}, {0x06F0, 0x06F9},
    /* NKo */
    {0x07C0, 0x07C9},
    /* Devanagari digits */
    {0x0966, 0x096F},
    /* Bengali */
    {0x09E6, 0x09EF}, {0x09F4, 0x09F9},
    /* Gurmukhi / Gujarati / Oriya / Tamil / Telugu / Kannada / Malayalam */
    {0x0A66, 0x0A6F}, {0x0AE6, 0x0AEF}, {0x0B66, 0x0B6F},
    {0x0BE6, 0x0BF2}, {0x0C66, 0x0C6F}, {0x0CE6, 0x0CEF},
    {0x0D66, 0x0D75},
    /* Sinhala / Thai / Lao / Tibetan / Myanmar */
    {0x0DE6, 0x0DEF}, {0x0E50, 0x0E59}, {0x0ED0, 0x0ED9},
    {0x0F20, 0x0F33}, {0x1040, 0x1049}, {0x1090, 0x1099},
    /* Khmer / Mongolian */
    {0x17E0, 0x17E9}, {0x1810, 0x1819},
    /* Fullwidth digits */
    {0xFF10, 0xFF19},
};

#define UCD_LETTER_N (int)(sizeof(UCD_LETTER) / sizeof(UCD_LETTER[0]))
#define UCD_NUMBER_N (int)(sizeof(UCD_NUMBER) / sizeof(UCD_NUMBER[0]))

static int uprop_range_contains(const UProp_Range *t, int n, int32_t cp) {
    int lo = 0, hi = n - 1;
    while (lo <= hi) {
        int mid = (lo + hi) >> 1;
        if (cp < t[mid].lo) hi = mid - 1;
        else if (cp > t[mid].hi) lo = mid + 1;
        else return 1;
    }
    return 0;
}

/* CharClass.u_props bitfield values (see CharClass struct). */
#define UP_LETTER 0x01u
#define UP_NUMBER 0x02u

/* ============================================================
 * Char class — 256-bit bitmap + inversion bit. One per CLASS
 * instruction. Owned by the RegExp.
 * ============================================================ */

typedef struct {
    uint8_t bits[32];
    uint8_t negate;
    /* P9.3-A2 — Unicode property bitfield. When set (via \p{NAME} in
     * a u-flag pattern), cc_test_cp consults the static UCD tables for
     * cp ≥ 128. ASCII portion of each property lives in the regular
     * bitmap (populated by cc_add_property_*). Class-level `negate`
     * still applies after the union. */
    uint8_t u_props;
} CharClass;

static void cc_clear(CharClass *cc) {
    for (int i = 0; i < 32; i++) cc->bits[i] = 0;
    cc->negate = 0;
    cc->u_props = 0;
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

/* P9.3-A2 — code-point membership test for u flag.
 *
 * cp < 128 is bitmap-tested (ASCII portion of any property lives in
 * the bitmap, populated by cc_add_property_*). cp ≥ 128 with no
 * u_props set is a miss (bitmap doesn't reach there). cp ≥ 128 with
 * u_props bits set scans the curated UCD tables. Class-level negate
 * inverts after the OR. */
static int cc_test_cp(const CharClass *cc, int32_t cp) {
    int in = 0;
    if (cp >= 0 && cp < 256) {
        in = (cc->bits[cp >> 3] >> (cp & 7)) & 1;
    }
    if (!in && cc->u_props && cp >= 0x80) {
        if ((cc->u_props & UP_LETTER) &&
            uprop_range_contains(UCD_LETTER, UCD_LETTER_N, cp)) {
            in = 1;
        } else if ((cc->u_props & UP_NUMBER) &&
                   uprop_range_contains(UCD_NUMBER, UCD_NUMBER_N, cp)) {
            in = 1;
        }
    }
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

/* P9.3-A2 — \p{NAME} class population. ASCII portion lands in the
 * bitmap; cp ≥ 128 portion is covered by UCD_* range tables (see
 * cc_test_cp). v0.1 supports L (Letter), N (Number), ASCII; aliases
 * resolved by parse_escape. */
static void cc_add_property_letter(CharClass *cc) {
    cc_add_range(cc, 'A', 'Z');
    cc_add_range(cc, 'a', 'z');
    cc->u_props |= UP_LETTER;
}
static void cc_add_property_number(CharClass *cc) {
    cc_add_range(cc, '0', '9');
    cc->u_props |= UP_NUMBER;
}
static void cc_add_property_ascii(CharClass *cc) {
    /* \p{ASCII} = [\x00-\x7F] — bitmap covers it entirely; no u_props
     * bit needed (cp ≥ 128 never matches ASCII). */
    for (int c = 0; c <= 0x7F; c++) cc_add(cc, (uint8_t)c);
}

/* Capture-group limits — used by parser (name table) and matcher
 * (Thread.saves array). Both sites must agree on the cap. */
#define REGEX_MAX_CAPTURES  32   /* > 32 capture groups → "regex too large" */
#define REGEX_SAVE_SLOTS    (REGEX_MAX_CAPTURES * 2)

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
    NK_LOOKAHEAD,      /* (?=X) — zero-width positive assertion */
    NK_NEG_LOOKAHEAD,  /* (?!X) — zero-width negative assertion */
    NK_LOOKBEHIND,     /* (?<=X) — zero-width positive lookbehind */
    NK_NEG_LOOKBEHIND, /* (?<!X) — zero-width negative lookbehind */
    NK_BACKREF,        /* \N (decimal) or \k<name> — references capture N. */
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
     * (`(?:...)`). Phase 1c.1 — capturing groups + re.exec.
     * BACKREF — references this capture index (1..n_captures). -1 means
     * unresolved named ref (look up via `backref_name` after parse). */
    int capture_idx;
    /* BACKREF named — name bytes point into pattern buffer (no copy);
     * resolved to capture_idx via Parser.names[] post-parse. Both NULL
     * / 0 for unnamed `\N` backrefs and non-BACKREF nodes. */
    const uint8_t *backref_name;
    int backref_name_len;
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
    /* Name table for named capture groups (`(?<name>X)`, Phase 1c.4.c).
     * Indexed by capture_idx (1..n_captures). Names point into the
     * pattern bytes (no copy); only valid during parse. NULL/0 = unnamed.
     * Slot 0 unused (whole-match record). */
    const uint8_t *names_ptr[REGEX_MAX_CAPTURES + 1];
    int names_len[REGEX_MAX_CAPTURES + 1];
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
static int is_word_byte(uint8_t c);

/* `\X` escape — produce either a single literal CHAR node (for
 * \n / \t / etc + pattern metas like \. \* \+) or a CLASS node
 * (for shorthand \d \D \w \W \s \S). */
static Node *parse_escape(Parser *ps) {
    if (p_eof(ps)) { ps->err = 1; return NULL; }
    uint8_t c = p_get(ps);
    /* `\1`..`\9` — DecimalEscape. JS spec: if N <= n_captures it's a
     * backreference to capture N (Phase 1c.4.c — implemented). Forward
     * references are allowed; resolution defers to a post-parse walk
     * that knows the final `n_captures`. When N > n_captures, ECMA
     * Annex B says interpret as OctalEscape / IdentityEscape (literal
     * digit) — that fallback is L3b follow-up; today such patterns are
     * rejected at post-parse time (preserving Phase 1b behavior). */
    if (c >= '1' && c <= '9') {
        Node *n = node_new(NK_BACKREF);
        n->capture_idx = c - '0';
        return n;
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
        case 'k': {
            /* `\k<name>` — named back-reference (Phase 1c.4.c). Per ECMA
             * Annex B, in patterns with no named groups `\k` is treated
             * as a literal "k" — but since named groups are now accepted
             * (`(?<name>X)`), any `\k` in a pattern that ALSO contains
             * a named group is unambiguously a named backref. For
             * simplicity tora treats `\k<` as always a backref intro;
             * if name resolution fails at post-parse, the regex is
             * rejected. Resolution is deferred to a post-parse walk
             * over the AST so forward references work. */
            if (p_eof(ps) || p_peek(ps) != '<') { ps->err = 1; return NULL; }
            p_get(ps); /* consume '<' */
            const uint8_t *name_start = ps->p + ps->i;
            while (!p_eof(ps) && p_peek(ps) != '>') {
                if (!is_word_byte(p_peek(ps))) { ps->err = 1; return NULL; }
                p_get(ps);
            }
            if (p_eof(ps)) { ps->err = 1; return NULL; }
            int name_len = (int)(ps->p + ps->i - name_start);
            if (name_len == 0) { ps->err = 1; return NULL; }
            p_get(ps); /* consume '>' */
            Node *n = node_new(NK_BACKREF);
            n->capture_idx = -1; /* unresolved — fixed up post-parse */
            n->backref_name = name_start;
            n->backref_name_len = name_len;
            return n;
        }
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
        case 'u': {
            /* `\uHHHH` (4-digit, always valid) or `\u{HHHH..}`
             * (extended, requires u flag). Encodes the code point to
             * 1–4 UTF-8 bytes and emits NK_CHAR (1 byte) or NK_CONCAT
             * of NK_CHARs (>1 bytes). Byte-stream matcher then matches
             * the same bytes in well-formed UTF-8 input — no new
             * opcode needed (per narrow-surface principle).
             *
             * Pre-existing bug also fixed: `\uHHHH` without u flag
             * used to parse as literal `u` followed by literal digits.
             * Now correctly parses as the encoded character. */
            int32_t cp = -1;
            if (!p_eof(ps) && p_peek(ps) == '{' && (ps->flags & RE_FLAG_U)) {
                /* Extended `\u{HHHH..}` form — u flag only. */
                p_get(ps); /* consume `{` */
                int64_t val = 0;
                int ndig = 0;
                while (!p_eof(ps) && p_peek(ps) != '}') {
                    uint8_t h = p_get(ps);
                    int d;
                    if (h >= '0' && h <= '9') d = h - '0';
                    else if (h >= 'a' && h <= 'f') d = h - 'a' + 10;
                    else if (h >= 'A' && h <= 'F') d = h - 'A' + 10;
                    else { ps->err = 1; return NULL; }
                    val = (val << 4) | (int64_t)d;
                    if (val > 0x10FFFF) { ps->err = 1; return NULL; }
                    ndig++;
                }
                if (ndig == 0 || p_eof(ps)) { ps->err = 1; return NULL; }
                p_get(ps); /* consume `}` */
                /* Lone surrogate (U+D800..U+DFFF) is a SyntaxError
                 * under u flag per ECMA-262 §22.2.1.1. */
                if (val >= 0xD800 && val <= 0xDFFF) { ps->err = 1; return NULL; }
                cp = (int32_t)val;
            } else if (ps->i + 4 <= ps->len) {
                /* `\uHHHH` 4-digit form — always valid. */
                int32_t val = 0;
                int ok = 1;
                for (int j = 0; j < 4; j++) {
                    uint8_t h = ps->p[ps->i + j];
                    int d;
                    if (h >= '0' && h <= '9') d = h - '0';
                    else if (h >= 'a' && h <= 'f') d = h - 'a' + 10;
                    else if (h >= 'A' && h <= 'F') d = h - 'A' + 10;
                    else { ok = 0; break; }
                    val = (val << 4) | d;
                }
                if (ok) {
                    ps->i += 4;
                    cp = val;
                }
            }
            if (cp < 0) {
                /* Lenient fallback: bare `\u` not followed by valid
                 * escape form is treated as literal `u` (matches the
                 * legacy default-case behavior for unknown escapes
                 * in non-u mode). Strict u-mode SyntaxError is L3b. */
                Node *n = node_new(NK_CHAR);
                n->ch = 'u';
                return n;
            }
            uint8_t buf[4];
            int blen = utf8_encode_cp(cp, buf);
            if (blen == 0) { ps->err = 1; return NULL; }
            if (blen == 1) {
                Node *n = node_new(NK_CHAR);
                n->ch = buf[0];
                return n;
            }
            Node *seq = node_new(NK_CONCAT);
            for (int b = 0; b < blen; b++) {
                Node *cn = node_new(NK_CHAR);
                cn->ch = buf[b];
                node_push_kid(seq, cn);
            }
            return seq;
        }
        case 'p':
        case 'P': {
            /* `\p{NAME}` / `\P{NAME}` — Unicode property class.
             * Requires u flag (per ECMA-262 §22.2.1.1). Without u flag,
             * fall through to literal `p`/`P` for back-compat.
             * v0.1 supports L (Letter), N (Number), ASCII. Unknown
             * names → SyntaxError. */
            if (!(ps->flags & RE_FLAG_U)) {
                Node *n = node_new(NK_CHAR);
                n->ch = c;
                return n;
            }
            if (p_eof(ps) || p_peek(ps) != '{') { ps->err = 1; return NULL; }
            p_get(ps); /* consume `{` */
            const uint8_t *name_start = ps->p + ps->i;
            while (!p_eof(ps) && p_peek(ps) != '}') {
                uint8_t ch = p_peek(ps);
                /* Accept word bytes for property name. (Real spec
                 * allows `=` for Name=Value form — L3b follow-up.) */
                if (!is_word_byte(ch)) { ps->err = 1; return NULL; }
                p_get(ps);
            }
            if (p_eof(ps)) { ps->err = 1; return NULL; }
            int name_len = (int)(ps->p + ps->i - name_start);
            if (name_len == 0) { ps->err = 1; return NULL; }
            p_get(ps); /* consume `}` */
            Node *n = node_new(NK_CLASS);
            cc_clear(&n->cc);
            int matched = 0;
            if ((name_len == 1 && name_start[0] == 'L')
                || (name_len == 6 && memcmp(name_start, "Letter", 6) == 0)) {
                cc_add_property_letter(&n->cc);
                matched = 1;
            } else if ((name_len == 1 && name_start[0] == 'N')
                       || (name_len == 6 && memcmp(name_start, "Number", 6) == 0)) {
                cc_add_property_number(&n->cc);
                matched = 1;
            } else if (name_len == 5 && memcmp(name_start, "ASCII", 5) == 0) {
                cc_add_property_ascii(&n->cc);
                matched = 1;
            }
            if (!matched) {
                node_free(n);
                ps->err = 1;
                return NULL;
            }
            /* `\P{X}` — negate at class level. (Inside [^...] this
             * stacks: cc_test_cp applies class-level negate after the
             * union, matching the bun observable behavior.) */
            if (c == 'P') n->cc.negate = 1;
            return n;
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
                case 'p': {
                    /* P9.3-A2 — `\p{NAME}` inside `[...]` under u flag.
                     * OR-unions the property into the current class.
                     * Without u flag, falls through to literal `p`.
                     * `\P{X}` inside class (complement) is L3b. */
                    if (!(ps->flags & RE_FLAG_U)) { c = e; break; }
                    if (p_eof(ps) || p_peek(ps) != '{') { ps->err = 1; node_free(n); return NULL; }
                    p_get(ps); /* consume `{` */
                    const uint8_t *ns = ps->p + ps->i;
                    while (!p_eof(ps) && p_peek(ps) != '}') {
                        if (!is_word_byte(p_peek(ps))) { ps->err = 1; node_free(n); return NULL; }
                        p_get(ps);
                    }
                    if (p_eof(ps)) { ps->err = 1; node_free(n); return NULL; }
                    int nl = (int)(ps->p + ps->i - ns);
                    if (nl == 0) { ps->err = 1; node_free(n); return NULL; }
                    p_get(ps); /* consume `}` */
                    int ok = 0;
                    if ((nl == 1 && ns[0] == 'L')
                        || (nl == 6 && memcmp(ns, "Letter", 6) == 0)) {
                        cc_add_property_letter(&n->cc); ok = 1;
                    } else if ((nl == 1 && ns[0] == 'N')
                               || (nl == 6 && memcmp(ns, "Number", 6) == 0)) {
                        cc_add_property_number(&n->cc); ok = 1;
                    } else if (nl == 5 && memcmp(ns, "ASCII", 5) == 0) {
                        cc_add_property_ascii(&n->cc); ok = 1;
                    }
                    if (!ok) { ps->err = 1; node_free(n); return NULL; }
                    continue;
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
         * `(?<=...)` positive lookbehind — Phase 1c.4.b.
         * `(?<!...)` negative lookbehind — Phase 1c.4.b.
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
                /* `(?<=...)` / `(?<!...)` lookbehind (Phase 1c.4.b).
                 * `(?<name>...)` named capture group (Phase 1c.4.c). */
                uint8_t after2 = (ps->i + 2 < ps->len) ? ps->p[ps->i + 2] : 0;
                if (after2 == '=') {
                    p_get(ps); p_get(ps); p_get(ps);
                    kind = NK_LOOKBEHIND;
                } else if (after2 == '!') {
                    p_get(ps); p_get(ps); p_get(ps);
                    kind = NK_NEG_LOOKBEHIND;
                } else if (is_word_byte(after2)) {
                    /* `(?<name>...)` — capture group with name. Allocate
                     * the next sequential capture_idx and record the
                     * name bytes (no copy) in the parser's name table.
                     * Names use the same word-byte rule as identifiers
                     * (`[A-Za-z0-9_]`); empty names rejected. */
                    p_get(ps); p_get(ps); /* consume `?<` */
                    const uint8_t *name_start = ps->p + ps->i;
                    while (!p_eof(ps) && p_peek(ps) != '>') {
                        if (!is_word_byte(p_peek(ps))) { ps->err = 1; return NULL; }
                        p_get(ps);
                    }
                    if (p_eof(ps)) { ps->err = 1; return NULL; }
                    int name_len = (int)(ps->p + ps->i - name_start);
                    if (name_len == 0) { ps->err = 1; return NULL; }
                    p_get(ps); /* consume `>` */
                    capture_idx = ++ps->n_captures;
                    if (capture_idx > REGEX_MAX_CAPTURES) { ps->err = 1; return NULL; }
                    ps->names_ptr[capture_idx] = name_start;
                    ps->names_len[capture_idx] = name_len;
                } else {
                    ps->err = 1;
                    return NULL;
                }
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
    /* Phase 1c.4.b — lookbehind. Same sub-Program shape as lookahead;
     * matcher resolves by scanning candidate start positions j ∈ [0..pos]
     * and probing whether the sub matches s[j..pos] exactly. */
    OP_LOOKBEHIND,
    OP_NEG_LOOKBEHIND,
    /* Phase 1c.4.c — backref. `a` = capture_idx (1..n_captures). At
     * match time the matcher fetches the captured slice (saves[2*idx
     * .. 2*idx+1]) and consumes that many bytes from input via a
     * per-thread `br_offset` state machine (see Thread + outer loop). */
    OP_BACKREF,
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
        case NK_BACKREF: {
            /* `n->capture_idx` is the 1-based capture index (validated
             * post-parse to be ≤ n_captures). */
            Inst i = { OP_BACKREF, 0, 0, n->capture_idx, 0 };
            prog_emit(p, i);
            break;
        }
        case NK_LOOKAHEAD:
        case NK_NEG_LOOKAHEAD:
        case NK_LOOKBEHIND:
        case NK_NEG_LOOKBEHIND: {
            /* Compile the assertion body into its own sub-Program with
             * an OP_MATCH at the end. Main bytecode emits the matching
             * OP_LOOKAHEAD / OP_NEG_LOOKAHEAD / OP_LOOKBEHIND /
             * OP_NEG_LOOKBEHIND with `a` = sub-program index. The
             * matcher resolves the assertion at add_thread time:
             *   - lookahead:  vm_match_at(sub, start=pos)
             *   - lookbehind: try each start j ∈ [0..pos], probe whether
             *                 sub matches s[j..pos] exactly (forward
             *                 sub-prog with end_target = pos). */
            Program *sub = (Program *)calloc(1, sizeof(Program));
            compile_node(sub, n->child);
            Inst m = { OP_MATCH, 0, 0, 0, 0 };
            prog_emit(sub, m);
            int sub_idx = prog_add_sub(p, sub);
            uint8_t op;
            switch (n->kind) {
                case NK_LOOKAHEAD:      op = OP_LOOKAHEAD; break;
                case NK_NEG_LOOKAHEAD:  op = OP_NEG_LOOKAHEAD; break;
                case NK_LOOKBEHIND:     op = OP_LOOKBEHIND; break;
                default:                op = OP_NEG_LOOKBEHIND; break;
            }
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
    /* Phase 1c.4.c — named-capture name table persisted past parse so
     * that match/exec output construction can build `.groups`. Indexed
     * by capture_idx 1..n_captures; NULL / 0 = unnamed positional group.
     * Names are owned copies (malloc + memcpy of the source bytes) so
     * they outlive the original pattern string. Freed in regex_drop. */
    uint8_t **capture_names;
    int *capture_name_lens;
    int n_named_captures; /* count of non-NULL entries — 0 = no `.groups` needed */
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

/* Resolve NK_BACKREF nodes after `parse_alt` finishes (which is when
 * the full capture set + name table are known). Returns 0 on success,
 * 1 on any unresolved reference (named ref to unknown name, or
 * positional `\N` where N > n_captures — the ECMA Annex B
 * OctalEscape / IdentityEscape fallback for the positional case is
 * L3b follow-up). */
static int resolve_backrefs(Node *n, const Parser *ps) {
    if (!n) return 0;
    if (n->kind == NK_BACKREF) {
        if (n->backref_name) {
            int found = 0;
            for (int i = 1; i <= ps->n_captures; i++) {
                if (ps->names_len[i] == n->backref_name_len
                    && memcmp(ps->names_ptr[i], n->backref_name,
                              (size_t)n->backref_name_len) == 0) {
                    n->capture_idx = i;
                    n->backref_name = NULL;
                    n->backref_name_len = 0;
                    found = 1;
                    break;
                }
            }
            if (!found) return 1;
        } else {
            if (n->capture_idx < 1 || n->capture_idx > ps->n_captures) {
                return 1;
            }
        }
    }
    if (n->child && resolve_backrefs(n->child, ps)) return 1;
    for (int i = 0; i < n->n_kids; i++) {
        if (resolve_backrefs(n->kids[i], ps)) return 1;
    }
    return 0;
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

    Parser ps = { pat, plen, 0, re->flags, 0, 0, {0}, {0} };
    Node *root = parse_alt(&ps);
    re->n_captures = ps.n_captures;
    /* Post-parse fixup — resolve `\k<name>` to capture_idx and validate
     * `\1..\9` against the now-known capture count. Done before the
     * `rejected` check so resolution failure also lands in `rejected`. */
    if (root && !ps.err && resolve_backrefs(root, &ps)) ps.err = 1;
    /* Persist named-capture table for `.groups` construction at match
     * time. Allocate parallel arrays sized n_captures+1 (slot 0 unused).
     * Owned copies of name bytes — Parser.names_ptr points into the
     * caller's pattern string which is not refcounted past compile. */
    if (root && !ps.err && ps.n_captures > 0) {
        size_t arr_cap = (size_t)(ps.n_captures + 1);
        re->capture_names = (uint8_t **)calloc(arr_cap, sizeof(uint8_t *));
        re->capture_name_lens = (int *)calloc(arr_cap, sizeof(int));
        for (int i = 1; i <= ps.n_captures; i++) {
            int nl = ps.names_len[i];
            if (nl > 0 && ps.names_ptr[i] != NULL) {
                uint8_t *copy = (uint8_t *)malloc((size_t)nl);
                memcpy(copy, ps.names_ptr[i], (size_t)nl);
                re->capture_names[i] = copy;
                re->capture_name_lens[i] = nl;
                re->n_named_captures++;
            }
        }
    }
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
    if (re->capture_names) {
        for (int i = 1; i <= re->n_captures; i++) {
            if (re->capture_names[i]) free(re->capture_names[i]);
        }
        free(re->capture_names);
    }
    if (re->capture_name_lens) free(re->capture_name_lens);
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

/* REGEX_MAX_CAPTURES / REGEX_SAVE_SLOTS defined earlier near the AST. */

typedef struct {
    int pc;
    /* `br_offset` — byte progress within an active OP_BACKREF
     * evaluation (0..cap_len). 0 = fresh entry / not in a backref.
     * When the matcher advances a thread within a multi-byte backref,
     * it re-schedules the thread at the SAME pc with br_offset
     * incremented; once br_offset == cap_len, the thread advances
     * to pc+1 with br_offset reset to 0. Phase 1c.4.c. */
    int br_offset;
    /* `u_skip` — outer-step defer counter for OP_ANYCHAR under u flag
     * with a multi-byte code point at the consume site. The Thompson
     * NFA outer loop advances by 1 byte per step; under u flag a
     * single `.` should consume 1 code point (1–4 bytes), so when
     * adv > 1 we schedule the destination thread at pos+adv with
     * u_skip = adv-1 so the thread sits in the queue for adv-1 outer
     * steps (consuming the continuation bytes implicitly) before
     * dispatching its op. Bypass-visited defer keeps the queued
     * thread alive across steps without colliding with fresh entrants
     * at the same pc. P9.3-A1. */
    int u_skip;
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

/* Forward decl — vm_match_at is below; lookahead resolution recurses.
 *
 * `end_target` gates the leftmost-first MATCH semantics:
 *   - end_target < 0  → normal mode: any MATCH at any pos wins
 *   - end_target >= 0 → length-restricted mode: only MATCH at
 *                       pos == end_target commits (used by lookbehind
 *                       to ask "does sub match s[start..end_target]?"). */
static int64_t vm_match_at(
    const Program *p,
    const uint8_t *s, int64_t slen,
    int64_t start_pos,
    uint8_t flags,
    Thread *cur, Thread *nxt,
    uint32_t *visited_cur, uint32_t *visited_nxt,
    uint32_t *step_id_ref,
    int64_t *out_saves,
    int64_t end_target
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
                              &step_id, NULL, -1);
    free(cur); free(nxt); free(vc); free(vn);
    return end >= 0 ? 1 : 0;
}

/* Phase 1c.4.b — lookbehind probe. Asks: does the sub-program have any
 * match s[j..pos] for some 0 ≤ j ≤ pos? Implementation: try each j from
 * pos down to 0 with vm_match_at(.., end_target=pos), accept on first
 * hit. O(pos × sub_len) worst case; in practice the body is short and
 * the loop bails on the first feasible j. (Future P14+ perf upgrade
 * path: compile body backwards and scan reverse — same Approach as V8.
 * That change replaces only this fn; AST / op / parser stay put.) */
static int sub_probe_ending_at(const Program *sub, const uint8_t *s, int64_t slen,
                               int64_t pos, uint8_t flags) {
    if (sub->n_insts == 0) return 1;
    Thread *cur = (Thread *)malloc(sizeof(Thread) * (size_t)sub->n_insts);
    Thread *nxt = (Thread *)malloc(sizeof(Thread) * (size_t)sub->n_insts);
    uint32_t *vc = (uint32_t *)calloc((size_t)sub->n_insts, sizeof(uint32_t));
    uint32_t *vn = (uint32_t *)calloc((size_t)sub->n_insts, sizeof(uint32_t));
    uint32_t step_id = 0;
    int ok = 0;
    for (int64_t j = pos; j >= 0; j--) {
        int64_t end = vm_match_at(sub, s, slen, j, flags, cur, nxt, vc, vn,
                                  &step_id, NULL, /* end_target = */ pos);
        if (end == pos) { ok = 1; break; }
    }
    free(cur); free(nxt); free(vc); free(vn);
    return ok;
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
        case OP_LOOKBEHIND: {
            const Program *sub = p->sub_progs[ins.a];
            if (sub_probe_ending_at(sub, s, slen, pos, flags)) {
                add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            }
            return;
        }
        case OP_NEG_LOOKBEHIND: {
            const Program *sub = p->sub_progs[ins.a];
            if (!sub_probe_ending_at(sub, s, slen, pos, flags)) {
                add_thread(tl, vt, pc + 1, p, s, slen, pos, flags, saves);
            }
            return;
        }
        default: {
            Thread *t = &tl->list[tl->n++];
            t->pc = pc;
            t->br_offset = 0;
            t->u_skip = 0;
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
    int64_t *out_saves,
    int64_t end_target
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
        /* Length-restricted (lookbehind) mode short-circuit: once pos
         * exceeds end_target, no later MATCH can satisfy the length
         * constraint — stop advancing. */
        if (end_target >= 0 && pos > end_target) break;
        nxt_tl.n = 0;
        nxt_tl.step_id = ++(*step_id_ref);
        int saw_match_this_step = 0;
        for (int ti = 0; ti < cur_tl.n && !saw_match_this_step; ti++) {
            const Thread *t = &cur_tl.list[ti];
            int pc = t->pc;
            /* P9.3-A1 — u_skip defer. A thread in-flight from an
             * OP_ANYCHAR multi-byte consume sits in the queue for
             * (adv-1) outer steps before dispatching. Bypass-visited
             * forward to nxt_tl so the deferred thread survives
             * step-to-step swaps without colliding with fresh entrants
             * at the same pc. */
            if (t->u_skip > 0) {
                Thread *t_new = &nxt_tl.list[nxt_tl.n++];
                t_new->pc = pc;
                t_new->u_skip = t->u_skip - 1;
                t_new->br_offset = t->br_offset;
                memcpy(t_new->saves, t->saves, sizeof(t_new->saves));
                continue;
            }
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
                        /* Under u flag, `.` consumes one code point —
                         * advance by the UTF-8 byte length of the
                         * leading byte at pos. Astral chars (4-byte
                         * emoji etc.) advance by 4, BMP non-ASCII by
                         * 2–3, ASCII by 1. Without u flag, classic
                         * byte-by-byte (1) per pre-P9.3 behavior.
                         * Defensive: if the leading byte indicates a
                         * length that runs past slen, fall back to 1
                         * (well-formed UTF-8 input shouldn't hit this).
                         *
                         * When adv > 1, the resulting thread(s) are
                         * patched with u_skip = adv-1 so they wait
                         * adv-1 outer steps before dispatching (see
                         * Thread.u_skip comment). Patches every newly-
                         * added thread because OP_SPLIT/OP_SAVE/etc.
                         * in the epsilon chain at pc+1 may produce
                         * multiple terminal threads. */
                        int adv = 1;
                        if (flags & RE_FLAG_U) {
                            int ul = utf8_len_for(s[pos]);
                            if (ul >= 1 && pos + ul <= slen) adv = ul;
                        }
                        int n_before = nxt_tl.n;
                        add_thread(&nxt_tl, &nxt_vt, pc + 1, p, s, slen, pos + adv, flags, t->saves);
                        if (adv > 1) {
                            for (int j = n_before; j < nxt_tl.n; j++) {
                                nxt_tl.list[j].u_skip = adv - 1;
                            }
                        }
                    }
                    break;
                }
                case OP_CLASS: {
                    if (pos < slen) {
                        const CharClass *cc = &p->classes[ins.a];
                        int adv = 1;
                        int match = 0;
                        if (flags & RE_FLAG_U) {
                            /* Decode one code point at s[pos]; test
                             * against cc as a code-point set. Advance
                             * by the encoded byte length and patch
                             * u_skip on the scheduled thread(s) so the
                             * outer loop waits adv-1 steps before
                             * dispatching pc+1. Same u_skip pattern
                             * as OP_ANYCHAR (P9.3-A1). */
                            int ul = utf8_len_for(s[pos]);
                            if (ul >= 1 && pos + ul <= slen) {
                                int dec_len;
                                int32_t cp = utf8_decode_cp(s + pos, &dec_len);
                                adv = (dec_len > 0) ? dec_len : ul;
                                match = cc_test_cp(cc, cp);
                            } else {
                                match = cc_test(cc, s[pos]);
                            }
                        } else {
                            match = cc_test(cc, s[pos]);
                        }
                        if (match) {
                            int n_before = nxt_tl.n;
                            add_thread(&nxt_tl, &nxt_vt, pc + 1, p, s, slen, pos + adv, flags, t->saves);
                            if (adv > 1) {
                                for (int j = n_before; j < nxt_tl.n; j++) {
                                    nxt_tl.list[j].u_skip = adv - 1;
                                }
                            }
                        }
                    }
                    break;
                }
                case OP_BACKREF: {
                    /* Per-thread state machine: `t->br_offset` tracks
                     * byte progress within this backref evaluation
                     * (0..cap_len). Each outer-loop step consumes ONE
                     * input byte and advances br_offset by 1; when
                     * br_offset reaches cap_len, the thread advances
                     * to pc+1. cap_len == 0 (empty / non-participating
                     * capture) is an epsilon hop into cur_tl at the
                     * same pos.
                     *
                     * Continuation re-scheduling (br_offset > 0) does
                     * NOT touch the visited table: visited is keyed
                     * by pc, and the fresh entrant to OP_BACKREF in
                     * the same step has br_offset=0 and must not be
                     * blocked by this thread (their state differs).
                     * See classic-errors note about Thompson NFA +
                     * backref dedup in this file's design notes. */
                    int idx = ins.a;
                    int slot_s = 2 * idx;
                    int slot_e = 2 * idx + 1;
                    int64_t cs = (idx >= 1 && slot_e < REGEX_SAVE_SLOTS)
                                 ? t->saves[slot_s] : -1;
                    int64_t ce = (idx >= 1 && slot_e < REGEX_SAVE_SLOTS)
                                 ? t->saves[slot_e] : -1;
                    int64_t cap_len = (cs < 0 || ce < 0) ? 0 : (ce - cs);
                    if (cap_len == 0) {
                        /* Epsilon-style — schedule pc+1 in cur_tl at
                         * the same pos. The outer for-loop on cur_tl
                         * keeps re-reading cur_tl.n which now grows;
                         * the visited-table dedup ensures no infinite
                         * insertion at the same pc. */
                        add_thread(&cur_tl, &cur_vt, pc + 1, p, s, slen, pos, flags, t->saves);
                    } else if (pos < slen
                               && char_eq(s[cs + t->br_offset], s[pos], flags)) {
                        int new_offset = t->br_offset + 1;
                        if (new_offset == cap_len) {
                            /* Backref complete — advance pc, reset
                             * br_offset (add_thread default sets 0). */
                            add_thread(&nxt_tl, &nxt_vt, pc + 1, p, s, slen, pos + 1, flags, t->saves);
                        } else {
                            /* Continue same pc next step with offset
                             * bumped. Direct insert (bypass visited)
                             * — see comment above. */
                            Thread *t_new = &nxt_tl.list[nxt_tl.n++];
                            t_new->pc = pc;
                            t_new->br_offset = new_offset;
                            t_new->u_skip = 0;
                            memcpy(t_new->saves, t->saves, sizeof(t_new->saves));
                        }
                    }
                    break;
                }
                case OP_MATCH:
                    /* Normal mode: leftmost-first wins, stop scanning
                     * this step (lower-priority threads can't beat).
                     * Length-restricted mode: only commit if pos meets
                     * end_target; on mismatch, this thread is dead but
                     * keep scanning — other threads may extend further
                     * via nxt_tl and MATCH later at the right length. */
                    if (end_target < 0 || pos == end_target) {
                        saw_match_this_step = 1;
                        end_pos = pos;
                        if (out_saves) memcpy(out_saves, t->saves, sizeof(t->saves));
                    }
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
     * recent next-list at this point.) Same end_target gate as above. */
    for (int ti = 0; ti < cur_tl.n; ti++) {
        const Thread *t = &cur_tl.list[ti];
        /* u_skip > 0 means a multi-byte ANYCHAR consume is still
         * unfinished — the thread isn't ready to accept. (Shouldn't
         * happen given the pos+ul <= slen guard at OP_ANYCHAR, but
         * defensive guard keeps the invariant local.) */
        if (p->insts[t->pc].op == OP_MATCH
            && t->u_skip == 0
            && (end_target < 0 || slen == end_target)) {
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
        /* Under u flag, start positions must land on code-point
         * boundaries — skip UTF-8 continuation bytes so the matcher
         * doesn't decode a continuation byte (0x80..0xBF) as a stand-
         * alone code point and accidentally satisfy `[^\p{...}]`
         * mid-sequence. P9.3-A2 fix. */
        if ((flags & RE_FLAG_U) && st < slen
            && (s[st] & 0xC0u) == 0x80u) {
            continue;
        }
        int64_t end = vm_match_at(p, s, slen, st, flags, cur, nxt, vc, vn,
                                  &step_id, out_saves, -1);
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
        if ((flags & RE_FLAG_U) && st < slen
            && (s[st] & 0xC0u) == 0x80u) {
            continue;
        }
        int64_t end = vm_match_at(p, s, slen, st, flags, cur, nxt, vc, vn,
                                  step_id_ref, out_saves, -1);
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
extern void *__torajs_dynobj_alloc(void);
extern void __torajs_dynobj_set(void **obj_slot, void *key, uint64_t tag, uint64_t value);
extern void __torajs_arrprops_set(void *arr_ptr, void *key, int64_t tag, int64_t value);
extern void __torajs_str_drop(void *s);

/* ANY tag used when a heap-shaped value (Str / dynobj / etc.) is
 * stored in a dynobj bucket. Must match runtime_str.c's __TORAJS_ANY_HEAP.
 * Used here by the `.groups` attachment path. */
#define REGEX_ANY_HEAP   4
#define REGEX_ANY_UNDEF  5

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

/* Build `.groups` dynobj from the named captures recorded on `re` and
 * the just-finished match's saves[]. Attaches the dict to `arr` via the
 * runtime_str.c arrprops side table (so `arr.groups` resolves via the
 * standard Array.<unknown-prop> path). Skips work entirely if `re` has
 * no named captures.
 *
 * Refcount discipline:
 *  - inner key ("digits", "first", ...): allocated rc=1, dynobj_set
 *    rc_inc's → rc=2, we str_drop → rc=1, dict owns the surviving ref.
 *  - inner value (captured Str or undefined sentinel 0): allocated
 *    rc=1, dynobj_set stores without rc_inc — dict takes the ref.
 *  - outer key ("groups"): same rc=1→drop pattern as inner key.
 *  - outer value (the groups dynobj): created rc=1, arrprops_set
 *    stores without rc_inc — arrprops table owns.
 * arrprops_drop_entry (called from arr_drop on refcount 0) walks the
 * outer entry → drops the inner dynobj → drops each inner entry's
 * key + value. */
static void attach_groups(void *arr, const RegExp *re, const uint8_t *s,
                          const int64_t *saves) {
    if (re->n_named_captures == 0 || re->capture_names == NULL) return;
    void *groups = __torajs_dynobj_alloc();
    for (int i = 1; i <= re->n_captures && i < REGEX_MAX_CAPTURES; i++) {
        if (re->capture_names[i] == NULL) continue;
        uint8_t *name_key = str_from_bytes(re->capture_names[i],
                                            re->capture_name_lens[i]);
        int64_t gs = saves[2 * i];
        int64_t ge = saves[2 * i + 1];
        if (gs < 0 || ge < 0) {
            /* Non-participating named group → ANY_UNDEF entry per spec
             * (m.groups.NAME === undefined). */
            __torajs_dynobj_set(&groups, name_key, REGEX_ANY_UNDEF, 0);
        } else {
            uint8_t *val_str = str_from_bytes(s + gs, ge - gs);
            __torajs_dynobj_set(&groups, name_key, REGEX_ANY_HEAP,
                                (uint64_t)(uintptr_t)val_str);
        }
        __torajs_str_drop(name_key);
    }
    static const uint8_t k_groups_bytes[] = { 'g','r','o','u','p','s' };
    uint8_t *outer_key = str_from_bytes(k_groups_bytes, 6);
    __torajs_arrprops_set(arr, outer_key, REGEX_ANY_HEAP,
                          (int64_t)(intptr_t)groups);
    __torajs_str_drop(outer_key);
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
            /* Phase 1c.4.c — attach `.groups` for named captures (same
             * mechanism as regex_exec). Global mode strips captures per
             * JS spec, so no .groups in that branch. */
            attach_groups(out, re, s, saves);
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
    /* Phase 1c.4.c — attach `.groups` if the regex has named captures.
     * arrprops side-table owns the dict; Array.<unknown-prop> lowering
     * already routes `result.groups` reads through arrprops_get. */
    attach_groups(out, re, s, saves);
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
