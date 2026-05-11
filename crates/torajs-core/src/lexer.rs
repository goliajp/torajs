//! Lexer — TS-shaped token stream. Subset for P0.2 (just enough for `console.log("hello")`).

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    String(String),
    Number(f64),
    /// T-25 — `BigInt` literal. Holds the lexeme's digit body
    /// (without the trailing `n` and without `0x`/`0b`/`0o` radix
    /// prefix), plus the radix it was written in. The runtime parses
    /// the digits at allocation time. Always non-negative — leading
    /// `-` is tokenized as a unary op.
    BigInt {
        digits: String,
        radix: u32,
    },
    // keywords
    Let,
    Const,
    If,
    Else,
    True,
    False,
    While,
    For,
    Break,
    Continue,
    Function,
    Return,
    /// `type Foo = { x: number }` declares a structural type alias.
    Type,
    /// M4 — exception handling.
    Try,
    Catch,
    Finally,
    Throw,
    /// M5.1 — class / new / this. Single-class no-inheritance subset:
    /// `class C { f: T; constructor(...) {...} method(...): R {...} }`.
    /// Class is desugared post-parse into a TypeDecl + a set of FnDecls,
    /// so `class` / `this` / `new` exist only at the parser layer.
    Class,
    New,
    This,
    /// M5.2 — single inheritance: `class Sub extends Base { ... }`.
    /// `super(args)` is only valid inside a subclass constructor; it
    /// desugars to a call to the parent's `__cm_Parent__ctor`.
    Extends,
    Super,
    // punctuation
    Dot,
    Comma,
    Colon,
    Semi,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Star,
    StarStar,
    StarStarEq,
    Slash,
    Percent,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    ShlShl,
    ShrShr,
    /// `>>>` — unsigned right shift (logical, no sign-extension). JS
    /// `(x >>> 0)` is the canonical Number→UInt32 coercion idiom; the
    /// parser maps it to `BinOp::UShr` which the SSA layer lowers to
    /// LLVM's `lshr` instruction.
    ShrShrShr,
    /// `/pattern/flags` — regex literal. JS lexer disambiguates `/`
    /// between division and the start of a regex by inspecting the
    /// previous token: regex if the prev is missing / a punctuator /
    /// a recognized keyword (return, typeof, ...), division otherwise.
    /// The pattern + flags are kept as raw strings; the parser wraps
    /// the token in `Expr::Regex { pattern, flags }`. Actual matching
    /// machinery is a follow-up phase — for now, ssa_lower rejects
    /// regex emission with a "regex matching not yet implemented"
    /// message.
    Regex {
        pattern: String,
        flags: String,
    },
    Bang,
    /// `~` — bitwise not.
    Tilde,
    /// `...` — spread (in array literal) or rest (in destructuring,
    /// function param). Currently only the array-literal spread is
    /// lowered.
    DotDotDot,
    /// `null` — the JS / TS null sentinel. tr lowers it to a 0
    /// pointer for any pointer-shaped slot (Str / Obj / Arr / Closure
    /// / FnSig); primitive-shaped slots (number / boolean) can't be
    /// nullable in this subset (would need a tag bit).
    Null,
    /// `??` — nullish coalescing. Desugars to a ternary on the LHS's
    /// nullability.
    QuestionQuestion,
    /// `?.` — optional chaining for member access. `obj?.field`
    /// desugars to `obj == null ? null : obj.field`.
    QuestionDot,
    /// Template literal `\`hi ${name} bye\`` — produced as a single
    /// token carrying the alternating literal segments and the
    /// pre-tokenized interpolation expressions. Parser stitches them
    /// into a `+` chain at AST build time. Interpolations may use
    /// arbitrary expressions but must NOT contain `}` outside of
    /// balanced `{}` pairs (no inner template strings yet).
    Template {
        parts: Vec<TemplatePart>,
    },
    /// `?` — start of a ternary `cond ? a : b` expression.
    Question,
    Eq,
    EqEqEq,
    BangEqEq,
    /// V3-18 m3 — JS loose equality `==` / `!=`. Parser maps to
    /// `BinOp::LooseEq` / `BinOp::LooseNeq`. Spec §7.2.13.
    EqEq,
    BangEq,
    /// `+=`, `-=`, `*=`, `/=`, `%=` — compound assignment, parser
    /// desugars to the corresponding binop + ordinary assign.
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    /// `++`, `--` — increment / decrement; both pre + post forms.
    /// Parser desugars to `x = x + 1` / `x = x - 1`. The post-form's
    /// "yield-old-value" semantic is approximated as "yield-new" for
    /// now (the most common JS use is in for-loop step where it is
    /// equivalent).
    PlusPlus,
    MinusMinus,
    /// `do { ... } while (cond);` — parses to `Stmt::DoWhile`.
    Do,
    /// `switch (x) { case v: ... default: ... }` — parses to
    /// `Stmt::Switch`.
    Switch,
    Case,
    Default,
    /// `typeof x` — yields a string literal at runtime.
    TypeOf,
    /// `void x` — evaluates `x` (for side effects) then yields `undefined`.
    /// Per JS spec §13.5.2; the standard idiom for "produce undefined" in
    /// pre-ES2020 code (`void 0`).
    Void,
    /// `x instanceof C` — relational operator. tr is statically typed,
    /// so this is a compile-time decision based on the LHS's static
    /// type vs the named class (and its superclass chain).
    InstanceOf,
    /// Phase J — `yield e` produces the next value of a `function*`
    /// generator. Recognized only inside generator bodies; desugar
    /// rewrites the surrounding fn into a class with a `next()` state
    /// machine.
    Yield,
    /// Phase L — `async function f()` declares an async function whose
    /// body returns a Promise. desugar_async wraps the body's return
    /// value in a Promise and switches the surface return type from
    /// `T` to `Promise<T>`.
    Async,
    /// Phase L — `await <expr>` extracts the resolved value from a
    /// Promise. MVP desugar at parse time: `await e` ⇒ `e.value`
    /// (synchronous read, only well-defined for already-fulfilled
    /// promises in the current eager-fire model).
    Await,
    /// Phase K — `import { a, b } from "./x"` / `import x from "./x"` /
    /// `import * as ns from "./x"`. Single-file mode treats the import
    /// as a syntax-only declaration (no symbol resolution); K.2-K.4 will
    /// wire in cross-file linking.
    Import,
    /// Phase K — `export function/class/type/const/let X` modifier on
    /// a declaration, or `export { a, b }` re-export form. Single-file
    /// mode strips the modifier (no semantic effect).
    Export,
    FatArrow,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Eof,
}

/// One slot inside a `Token::Template`. Either a raw literal segment
/// (the bytes between backticks / `${` / `}`) or a pre-tokenized
/// interpolation expression (everything inside `${…}`). The parser at
/// the Token::Template arm stitches them into `lit0 + expr0 + lit1 + …`.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    Lit(String),
    Expr(Vec<Spanned>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub token: Token,
    pub span: Span,
}

pub fn tokenize(src: &str) -> Result<Vec<Spanned>, String> {
    let bytes = src.as_bytes();
    let len = bytes.len() as u32;
    let mut out = Vec::new();
    let mut i: u32 = 0;

    while i < len {
        let start = i;
        let b = bytes[i as usize];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            b'.' => {
                // `...` (spread/rest) emits a single DotDotDot token.
                // Bare `.` stays Dot for member access.
                if peek(bytes, i + 1) == Some(b'.') && peek(bytes, i + 2) == Some(b'.') {
                    i += 3;
                    emit(&mut out, Token::DotDotDot, start, i);
                } else {
                    emit(&mut out, Token::Dot, start, advance(&mut i));
                }
            }
            b',' => emit(&mut out, Token::Comma, start, advance(&mut i)),
            b':' => emit(&mut out, Token::Colon, start, advance(&mut i)),
            b';' => emit(&mut out, Token::Semi, start, advance(&mut i)),
            b'(' => emit(&mut out, Token::LParen, start, advance(&mut i)),
            b')' => emit(&mut out, Token::RParen, start, advance(&mut i)),
            b'{' => emit(&mut out, Token::LBrace, start, advance(&mut i)),
            b'}' => emit(&mut out, Token::RBrace, start, advance(&mut i)),
            b'[' => emit(&mut out, Token::LBracket, start, advance(&mut i)),
            b']' => emit(&mut out, Token::RBracket, start, advance(&mut i)),
            b'+' => {
                i += 1;
                if peek(bytes, i) == Some(b'+') {
                    i += 1;
                    emit(&mut out, Token::PlusPlus, start, i);
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::PlusEq, start, i);
                } else {
                    emit(&mut out, Token::Plus, start, i);
                }
            }
            b'-' => {
                i += 1;
                if peek(bytes, i) == Some(b'-') {
                    i += 1;
                    emit(&mut out, Token::MinusMinus, start, i);
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::MinusEq, start, i);
                } else {
                    emit(&mut out, Token::Minus, start, i);
                }
            }
            b'*' => {
                i += 1;
                /* V3-01 — `**` exponent operator (and its compound
                 * assign `**=`). JS spec: right-associative,
                 * precedence higher than mul / div / mod. */
                if peek(bytes, i) == Some(b'*') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::StarStarEq, start, i);
                    } else {
                        emit(&mut out, Token::StarStar, start, i);
                    }
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::StarEq, start, i);
                } else {
                    emit(&mut out, Token::Star, start, i);
                }
            }
            b'~' => emit(&mut out, Token::Tilde, start, advance(&mut i)),
            b'?' => {
                // `?` (ternary), `??` (nullish coalescing), `?.`
                // (optional chaining). Single-char emit becomes
                // multi-char when the suffix is `?` or `.`.
                if peek(bytes, i + 1) == Some(b'?') {
                    i += 2;
                    emit(&mut out, Token::QuestionQuestion, start, i);
                } else if peek(bytes, i + 1) == Some(b'.') {
                    i += 2;
                    emit(&mut out, Token::QuestionDot, start, i);
                } else {
                    emit(&mut out, Token::Question, start, advance(&mut i));
                }
            }
            b'/' => {
                // `//` line comment, `/* */` block comment, regex
                // literal, or division. Disambiguation between regex
                // and division uses the previous token: regex when prev
                // is None / a punctuator / a keyword that can start an
                // expression on its right.
                match peek(bytes, i + 1) {
                    Some(b'/') => {
                        // Line comment — consume to end-of-line / EOF.
                        i += 2;
                        while i < len && bytes[i as usize] != b'\n' {
                            i += 1;
                        }
                        // Don't consume the newline itself — outer loop's
                        // whitespace branch handles it (and any trailing
                        // \r\n line ending).
                    }
                    Some(b'*') => {
                        // Block comment — consume to first `*/`. Nested
                        // block comments are NOT supported (TS doesn't
                        // support them either; matches `tsc` / `bun`).
                        i += 2;
                        let comment_start = start;
                        loop {
                            if i + 1 >= len {
                                return Err(format!(
                                    "unterminated block comment starting at {comment_start}"
                                ));
                            }
                            if bytes[i as usize] == b'*' && bytes[(i + 1) as usize] == b'/' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ if regex_context(out.last().map(|s| &s.token)) => {
                        // Scan a regex literal: `/pattern/flags`.
                        // Pattern body: read until an unescaped `/`,
                        // honoring `\\.` escapes and `[...]` character
                        // classes (where `/` is allowed bare).
                        let body_start = (i + 1) as usize;
                        let mut p = body_start;
                        let mut in_class = false;
                        loop {
                            if p >= len as usize {
                                return Err(format!(
                                    "unterminated regex literal starting at {start}"
                                ));
                            }
                            let c = bytes[p];
                            if c == b'\n' {
                                return Err(format!(
                                    "unterminated regex literal at {start} (line break before closing `/`)"
                                ));
                            }
                            if c == b'\\' {
                                // Skip the escape sequence's next byte.
                                p += 2;
                                continue;
                            }
                            if c == b'[' {
                                in_class = true;
                                p += 1;
                                continue;
                            }
                            if c == b']' && in_class {
                                in_class = false;
                                p += 1;
                                continue;
                            }
                            if c == b'/' && !in_class {
                                break;
                            }
                            p += 1;
                        }
                        let pattern =
                            String::from_utf8_lossy(&bytes[body_start..p]).into_owned();
                        // Flags: any trailing ASCII letters.
                        let flags_start = p + 1;
                        let mut q = flags_start;
                        while q < len as usize && bytes[q].is_ascii_alphabetic() {
                            q += 1;
                        }
                        let flags =
                            String::from_utf8_lossy(&bytes[flags_start..q]).into_owned();
                        i = q as u32;
                        emit(&mut out, Token::Regex { pattern, flags }, start, i);
                    }
                    Some(b'=') => {
                        i += 2;
                        emit(&mut out, Token::SlashEq, start, i);
                    }
                    _ => emit(&mut out, Token::Slash, start, advance(&mut i)),
                }
            }
            b'%' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::PercentEq, start, i);
                } else {
                    emit(&mut out, Token::Percent, start, i);
                }
            }
            b'&' => {
                i += 1;
                if peek(bytes, i) == Some(b'&') {
                    i += 1;
                    emit(&mut out, Token::AmpAmp, start, i);
                } else {
                    emit(&mut out, Token::Amp, start, i);
                }
            }
            b'|' => {
                i += 1;
                if peek(bytes, i) == Some(b'|') {
                    i += 1;
                    emit(&mut out, Token::PipePipe, start, i);
                } else {
                    emit(&mut out, Token::Pipe, start, i);
                }
            }
            b'^' => emit(&mut out, Token::Caret, start, advance(&mut i)),
            b'<' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::LtEq, start, i);
                } else if peek(bytes, i) == Some(b'<') {
                    i += 1;
                    emit(&mut out, Token::ShlShl, start, i);
                } else {
                    emit(&mut out, Token::Lt, start, i);
                }
            }
            b'>' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::GtEq, start, i);
                } else if peek(bytes, i) == Some(b'>') {
                    i += 1;
                    if peek(bytes, i) == Some(b'>') {
                        i += 1;
                        emit(&mut out, Token::ShrShrShr, start, i);
                    } else {
                        emit(&mut out, Token::ShrShr, start, i);
                    }
                } else {
                    emit(&mut out, Token::Gt, start, i);
                }
            }
            b'=' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::EqEqEq, start, i);
                    } else {
                        // V3-18 m3 — `==` IsLooselyEqual per §7.2.13.
                        // Restored from "out-of-scope" 2026-05-10
                        // (test262 100% bar). Emits a new
                        // Token::EqEq → BinOp::LooseEq.
                        emit(&mut out, Token::EqEq, start, i);
                    }
                } else if peek(bytes, i) == Some(b'>') {
                    i += 1;
                    emit(&mut out, Token::FatArrow, start, i);
                } else {
                    emit(&mut out, Token::Eq, start, i);
                }
            }
            b'!' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::BangEqEq, start, i);
                    } else {
                        // V3-18 m3 — `!=` is `!IsLooselyEqual`.
                        emit(&mut out, Token::BangEq, start, i);
                    }
                } else {
                    // Unary logical not — used as `!cond`. M1.5.
                    emit(&mut out, Token::Bang, start, i);
                }
            }
            b'"' | b'\'' => {
                let quote = bytes[i as usize];
                i += 1;
                // Decode JS-style escape sequences. Supported: \\ \" \'
                // \n \r \t \b \f \v \0 \xNN \uNNNN \u{NNNN...}.
                // Unknown escapes pass through their letter (matches
                // V8's annex-B-friendly behavior for the small subset
                // our tests need).
                let mut buf: Vec<u8> = Vec::new();
                while i < len && bytes[i as usize] != quote {
                    let c = bytes[i as usize];
                    if c == b'\\' && i + 1 < len {
                        let esc = bytes[i as usize + 1];
                        match esc {
                            b'n' => { buf.push(b'\n'); i += 2; continue; }
                            b'r' => { buf.push(b'\r'); i += 2; continue; }
                            b't' => { buf.push(b'\t'); i += 2; continue; }
                            b'b' => { buf.push(0x08); i += 2; continue; }
                            b'f' => { buf.push(0x0c); i += 2; continue; }
                            b'v' => { buf.push(0x0b); i += 2; continue; }
                            b'0' => { buf.push(0); i += 2; continue; }
                            b'\\' => { buf.push(b'\\'); i += 2; continue; }
                            b'\'' => { buf.push(b'\''); i += 2; continue; }
                            b'"' => { buf.push(b'"'); i += 2; continue; }
                            b'`' => { buf.push(b'`'); i += 2; continue; }
                            // V3-18 m1.h.33 — `\xNN` hex escape (2 hex
                            // digits → byte). Per JS spec §12.8.4.1
                            // HexEscapeSequence.
                            b'x' if i + 3 < len
                                && bytes[i as usize + 2].is_ascii_hexdigit()
                                && bytes[i as usize + 3].is_ascii_hexdigit() => {
                                let hi = (bytes[i as usize + 2] as char).to_digit(16).unwrap();
                                let lo = (bytes[i as usize + 3] as char).to_digit(16).unwrap();
                                let cp = (hi * 16 + lo) as u32;
                                push_codepoint(&mut buf, cp);
                                i += 4;
                                continue;
                            }
                            // V3-18 m1.h.33 — `\uNNNN` 4-digit unicode
                            // escape. Per JS spec §12.8.4.1 UnicodeEscapeSequence.
                            b'u' if i + 5 < len
                                && bytes[i as usize + 2].is_ascii_hexdigit()
                                && bytes[i as usize + 3].is_ascii_hexdigit()
                                && bytes[i as usize + 4].is_ascii_hexdigit()
                                && bytes[i as usize + 5].is_ascii_hexdigit() => {
                                let mut cp: u32 = 0;
                                for k in 2..=5 {
                                    cp = cp * 16
                                        + (bytes[i as usize + k] as char).to_digit(16).unwrap();
                                }
                                push_codepoint(&mut buf, cp);
                                i += 6;
                                continue;
                            }
                            // `\u{N...N}` extended form (1-6 hex
                            // digits). Per JS spec §12.8.4.1 LegacyOctalEscape
                            // not handled; ES2015+ form only.
                            b'u' if i + 3 < len && bytes[i as usize + 2] == b'{' => {
                                let mut k = i as usize + 3;
                                let mut cp: u32 = 0;
                                let mut digits = 0;
                                while k < len as usize && bytes[k].is_ascii_hexdigit() && digits < 6 {
                                    cp = cp * 16 + (bytes[k] as char).to_digit(16).unwrap();
                                    k += 1;
                                    digits += 1;
                                }
                                if digits >= 1 && k < len as usize && bytes[k] == b'}' {
                                    push_codepoint(&mut buf, cp);
                                    i = (k + 1) as u32;
                                    continue;
                                }
                                // malformed → fall through to passthrough
                                buf.push(esc);
                                i += 2;
                                continue;
                            }
                            other => { buf.push(other); i += 2; continue; }
                        }
                    }
                    buf.push(c);
                    i += 1;
                }
                if i >= len {
                    return Err(format!("unterminated string starting at {start}"));
                }
                let value = String::from_utf8(buf)
                    .map_err(|_| format!("invalid utf-8 in string at {start}"))?;
                i += 1; // consume closing quote
                emit(&mut out, Token::String(value), start, i);
            }
            b'`' => {
                // Template literal. Read alternating literal segments
                // and `${...}` interpolations until the closing
                // backtick. Each interpolation's source slice is
                // recursively tokenized so the parser can drop a
                // sub-Parser into it without re-doing lex.
                //
                // Limitation: interpolations track only `{` `}` depth,
                // not strings or backticks inside the expression. So
                // `${ "}" }` (a literal `}` inside a string) and
                // nested templates `${\`...\`}` aren't supported. The
                // common arithmetic / member-access shapes work fine.
                i += 1; // consume opening backtick
                let mut parts: Vec<TemplatePart> = Vec::new();
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    if i >= len {
                        return Err(format!(
                            "unterminated template literal starting at {start}"
                        ));
                    }
                    let b = bytes[i as usize];
                    if b == b'`' {
                        if !buf.is_empty() || parts.is_empty() {
                            let s = std::str::from_utf8(&buf)
                                .map_err(|_| format!("invalid utf-8 in template at {start}"))?
                                .to_string();
                            parts.push(TemplatePart::Lit(s));
                        }
                        i += 1; // consume closing backtick
                        break;
                    }
                    if b == b'$' && peek(bytes, i + 1) == Some(b'{') {
                        // Flush literal segment (even if empty — we
                        // need the alternation).
                        let s = std::str::from_utf8(&buf)
                            .map_err(|_| format!("invalid utf-8 in template at {start}"))?
                            .to_string();
                        parts.push(TemplatePart::Lit(s));
                        buf.clear();
                        i += 2; // consume `${`
                        let expr_start = i;
                        let mut depth: i32 = 1;
                        while i < len && depth > 0 {
                            match bytes[i as usize] {
                                b'{' => depth += 1,
                                b'}' => depth -= 1,
                                _ => {}
                            }
                            if depth == 0 {
                                break;
                            }
                            i += 1;
                        }
                        if i >= len {
                            return Err(format!(
                                "unterminated template `${{...}}` interpolation at {start}"
                            ));
                        }
                        let expr_end = i;
                        i += 1; // consume `}`
                        let expr_src =
                            std::str::from_utf8(&bytes[expr_start as usize..expr_end as usize])
                                .map_err(|_| {
                                    format!("invalid utf-8 in template interp at {start}")
                                })?;
                        let inner = tokenize(expr_src)?;
                        // Keep the trailing Eof so the sub-Parser's
                        // peek() never falls off the end (its expr
                        // parsers rely on the Eof guard).
                        parts.push(TemplatePart::Expr(inner));
                        continue;
                    }
                    buf.push(b);
                    i += 1;
                }
                emit(&mut out, Token::Template { parts }, start, i);
            }
            b if is_ident_start(b) => {
                while i < len && is_ident_cont(bytes[i as usize]) {
                    i += 1;
                }
                let name = std::str::from_utf8(&bytes[start as usize..i as usize])
                    .expect("ascii ident slice is valid utf-8");
                let token = match name {
                    "let" => Token::Let,
                    "const" => Token::Const,
                    // V3-18 m4 first wedge — `var` lexes as Let.
                    // Full hoisting + function-scope semantics
                    // (vs let/const block-scope) is a follow-up;
                    // many test262 cases use `var` for plain
                    // top-level declarations and just need it to
                    // parse + behave like let. Programs that depend
                    // on hoisting to use `var` before its decl will
                    // continue to fail until the m4.b hoisting pass.
                    "var" => Token::Let,
                    "if" => Token::If,
                    "else" => Token::Else,
                    "true" => Token::True,
                    "false" => Token::False,
                    "while" => Token::While,
                    "for" => Token::For,
                    "break" => Token::Break,
                    "continue" => Token::Continue,
                    "function" => Token::Function,
                    "return" => Token::Return,
                    "type" => Token::Type,
                    "try" => Token::Try,
                    "catch" => Token::Catch,
                    "finally" => Token::Finally,
                    "throw" => Token::Throw,
                    "class" => Token::Class,
                    "new" => Token::New,
                    "this" => Token::This,
                    "extends" => Token::Extends,
                    "super" => Token::Super,
                    "do" => Token::Do,
                    "switch" => Token::Switch,
                    "case" => Token::Case,
                    "default" => Token::Default,
                    "typeof" => Token::TypeOf,
                    "void" => Token::Void,
                    "instanceof" => Token::InstanceOf,
                    "yield" => Token::Yield,
                    "async" => Token::Async,
                    "await" => Token::Await,
                    "import" => Token::Import,
                    "export" => Token::Export,
                    // `from` and `as` are contextual keywords in TS —
                    // they may appear as plain identifiers outside
                    // import context (`let from = 1` is legal). Lexer
                    // keeps them as Ident; parser recognizes them by
                    // string match in the import-decl tail.
                    "null" => Token::Null,
                    _ => Token::Ident(name.to_string()),
                };
                emit(&mut out, token, start, i);
            }
            b if b.is_ascii_digit() => {
                // V3-18 m1.h.55 — `0b...` binary and `0o...` octal
                // literals per JS spec §12.8.3. Both lex as base-2 / -8
                // u64, then cast to f64 (matching the existing 0x...
                // path). Same `n` BigInt suffix support.
                if b == b'0'
                    && peek(bytes, i + 1).is_some_and(|c| c == b'b' || c == b'B')
                {
                    i += 2;
                    let dig_start = i;
                    while i < len
                        && (bytes[i as usize] == b'0'
                            || bytes[i as usize] == b'1'
                            || bytes[i as usize] == b'_')
                    {
                        i += 1;
                    }
                    if i == dig_start {
                        return Err(format!("invalid binary literal at {start}"));
                    }
                    let raw = std::str::from_utf8(&bytes[dig_start as usize..i as usize])
                        .expect("ascii bin digits are valid utf-8");
                    let cleaned;
                    let s: &str = if raw.contains('_') { cleaned = raw.replace('_', ""); &cleaned } else { raw };
                    let n: u64 = u64::from_str_radix(s, 2)
                        .map_err(|_| format!("invalid binary number at {start}"))?;
                    emit(&mut out, Token::Number(n as f64), start, i);
                    continue;
                }
                if b == b'0'
                    && peek(bytes, i + 1).is_some_and(|c| c == b'o' || c == b'O')
                {
                    i += 2;
                    let dig_start = i;
                    while i < len
                        && ((bytes[i as usize] >= b'0' && bytes[i as usize] <= b'7')
                            || bytes[i as usize] == b'_')
                    {
                        i += 1;
                    }
                    if i == dig_start {
                        return Err(format!("invalid octal literal at {start}"));
                    }
                    let raw = std::str::from_utf8(&bytes[dig_start as usize..i as usize])
                        .expect("ascii oct digits are valid utf-8");
                    let cleaned;
                    let s: &str = if raw.contains('_') { cleaned = raw.replace('_', ""); &cleaned } else { raw };
                    let n: u64 = u64::from_str_radix(s, 8)
                        .map_err(|_| format!("invalid octal number at {start}"))?;
                    emit(&mut out, Token::Number(n as f64), start, i);
                    continue;
                }
                // 0x... hex literal — TS / JS standard. Parse as u64 and
                // cast to f64; values up to 2^53 round-trip exactly,
                // which covers every realistic bitwise / mask use.
                if b == b'0'
                    && peek(bytes, i + 1).is_some_and(|c| c == b'x' || c == b'X')
                {
                    i += 2; // skip "0x"
                    let hex_start = i;
                    while i < len
                        && (bytes[i as usize].is_ascii_hexdigit() || bytes[i as usize] == b'_')
                    {
                        i += 1;
                    }
                    if i == hex_start {
                        return Err(format!("invalid hex literal at {start}"));
                    }
                    let raw = std::str::from_utf8(&bytes[hex_start as usize..i as usize])
                        .expect("ascii hex digits are valid utf-8");
                    let cleaned;
                    let s: &str = if raw.contains('_') { cleaned = raw.replace('_', ""); &cleaned } else { raw };
                    /* T-25 BigInt: `0x...n`. Hex-radix BigInt literal. */
                    if peek(bytes, i) == Some(b'n') {
                        let digits = s.to_string();
                        i += 1;
                        emit(&mut out, Token::BigInt { digits, radix: 16 }, start, i);
                        continue;
                    }
                    let n: u64 = u64::from_str_radix(s, 16)
                        .map_err(|_| format!("invalid hex number at {start}"))?;
                    emit(&mut out, Token::Number(n as f64), start, i);
                    continue;
                }
                // V3-18 m1.h.55 — numeric separator `_` (per JS spec
                // §12.8.3 NumericLiteralSeparator). Stripped before
                // parsing. Allowed between digits only; consecutive
                // `_` or leading/trailing `_` aren't valid but our
                // tolerant parse silently allows them — strict spec
                // rejection is a polish item.
                while i < len
                    && (bytes[i as usize].is_ascii_digit() || bytes[i as usize] == b'_')
                {
                    i += 1;
                }
                if peek(bytes, i) == Some(b'.')
                    && peek(bytes, i + 1).is_some_and(|c| c.is_ascii_digit())
                {
                    i += 1;
                    while i < len
                        && (bytes[i as usize].is_ascii_digit() || bytes[i as usize] == b'_')
                    {
                        i += 1;
                    }
                } else if peek(bytes, i) == Some(b'.')
                    && peek(bytes, i + 1) == Some(b'.')
                {
                    // V3-18 m1.h.21 — `0..toString()` form. JS spec
                    // §12.8.3 allows DecimalLiteral to end with a
                    // trailing `.`; the second `.` then begins a
                    // member access. Without consuming the first
                    // dot here, the parser sees `Number(0)` then
                    // `.` `.` and bails. Used by 20+ test262 cases
                    // (Number/prototype/toString/numeric-literal-*)
                    // and the standard idiom for `(123).toString()`
                    // without parens.
                    i += 1;
                }
                // Scientific notation: `e` / `E` optionally followed by
                // `+` / `-`, then one or more digits. Only consume when
                // the suffix is a real exponent — `1eFoo` parses as the
                // number `1` followed by the ident `eFoo`.
                if (peek(bytes, i) == Some(b'e') || peek(bytes, i) == Some(b'E'))
                    && {
                        let mut j = i + 1;
                        if peek(bytes, j) == Some(b'+') || peek(bytes, j) == Some(b'-') {
                            j += 1;
                        }
                        peek(bytes, j).is_some_and(|c| c.is_ascii_digit())
                    }
                {
                    i += 1;
                    if peek(bytes, i) == Some(b'+') || peek(bytes, i) == Some(b'-') {
                        i += 1;
                    }
                    while i < len && bytes[i as usize].is_ascii_digit() {
                        i += 1;
                    }
                }
                let raw = std::str::from_utf8(&bytes[start as usize..i as usize])
                    .expect("ascii digits are valid utf-8");
                // V3-18 m1.h.55 — strip numeric separators before
                // parsing into f64 / BigInt.
                let s_owned;
                let s: &str = if raw.contains('_') {
                    s_owned = raw.replace('_', "");
                    &s_owned
                } else {
                    raw
                };
                /* T-25 BigInt: `<integer>n` literal. Only matches when
                 * the lexeme has no `.` or `e/E` (decimal-only integer)
                 * and is followed by `n`. JS rejects `1.5n` / `1e2n`
                 * at parse time — same here: the `n` falls through to
                 * the unexpected-byte branch above on a fractional
                 * literal. */
                if peek(bytes, i) == Some(b'n')
                    && !s.contains('.')
                    && !s.contains('e')
                    && !s.contains('E')
                {
                    let digits = s.to_string();
                    i += 1;
                    emit(&mut out, Token::BigInt { digits, radix: 10 }, start, i);
                    continue;
                }
                let n: f64 = s
                    .parse()
                    .map_err(|_| format!("invalid number at {start}"))?;
                emit(&mut out, Token::Number(n), start, i);
            }
            _ => return Err(format!("unexpected byte {b:#x} at {start}")),
        }
    }
    emit(&mut out, Token::Eof, len, len);
    Ok(out)
}

fn advance(i: &mut u32) -> u32 {
    *i += 1;
    *i
}

fn peek(bytes: &[u8], i: u32) -> Option<u8> {
    bytes.get(i as usize).copied()
}

/// JS lexer ambiguity: `/` is a regex-literal start when the previous
/// token is a punctuator that can begin an expression on its right
/// or a keyword like `return` / `typeof` / etc.; otherwise it's a
/// division operator. Mirrors what V8 / SpiderMonkey / JSC do.
fn regex_context(prev: Option<&Token>) -> bool {
    let Some(t) = prev else {
        // Start of file — anything goes; default-yes.
        return true;
    };
    matches!(
        t,
        // Punctuators
        Token::LParen
            | Token::LBrace
            | Token::LBracket
            | Token::Comma
            | Token::Semi
            | Token::Colon
            | Token::Question
            | Token::QuestionQuestion
            | Token::QuestionDot
            | Token::Bang
            | Token::Tilde
            | Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
            | Token::Percent
            | Token::Eq
            | Token::EqEqEq
            | Token::BangEqEq
            | Token::EqEq
            | Token::BangEq
            | Token::Lt
            | Token::Gt
            | Token::LtEq
            | Token::GtEq
            | Token::Amp
            | Token::AmpAmp
            | Token::Pipe
            | Token::PipePipe
            | Token::Caret
            | Token::ShlShl
            | Token::ShrShr
            | Token::ShrShrShr
            | Token::FatArrow
            | Token::DotDotDot
            | Token::SlashEq
            | Token::PlusEq
            | Token::MinusEq
            | Token::StarEq
            | Token::PercentEq
            // Expression-starting keywords
            | Token::Return
            | Token::TypeOf
            | Token::Void
            | Token::InstanceOf
            | Token::New
            | Token::Throw
            | Token::Case
            | Token::Yield
            | Token::Await
            | Token::Else
            | Token::Do
            | Token::If
            | Token::While
            | Token::For
    )
}

fn emit(out: &mut Vec<Spanned>, token: Token, start: u32, end: u32) {
    out.push(Spanned {
        token,
        span: Span { start, end },
    });
}

/// Encode a Unicode code point as UTF-8 into `buf`. Used by string-
/// literal escape decoding (`\xNN`, `\uNNNN`, `\u{N...N}`). Codepoints
/// past U+10FFFF or in the surrogate range fall back to U+FFFD
/// REPLACEMENT CHARACTER — matches V8's recovery behavior on malformed
/// escapes.
fn push_codepoint(buf: &mut Vec<u8>, cp: u32) {
    let c = char::from_u32(cp).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    let s = c.encode_utf8(&mut tmp);
    buf.extend_from_slice(s.as_bytes());
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
