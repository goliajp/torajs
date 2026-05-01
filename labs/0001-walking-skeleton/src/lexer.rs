//! Lexer — TS-shaped token stream. Subset for P0.2 (just enough for `console.log("hello")`).

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    String(String),
    Number(f64),
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
    Slash,
    Percent,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    ShlShl,
    ShrShr,
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
                if peek(bytes, i) == Some(b'=') {
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
                // `//` line comment, `/* */` block comment, or division.
                // TS grammar puts comments at the lexer level (whitespace-
                // equivalent); we skip past them without emitting a token.
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
                    emit(&mut out, Token::ShrShr, start, i);
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
                        return Err(format!(
                            "`==` is not supported, use `===` (strict equality) at {start}"
                        ));
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
                        return Err(format!(
                            "`!=` is not supported, use `!==` (strict inequality) at {start}"
                        ));
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
                // \n \r \t \b \f \v \0. Unknown escapes pass through
                // their letter (matches V8's annex-B-friendly behavior
                // for the small subset our tests need).
                let mut buf: Vec<u8> = Vec::new();
                while i < len && bytes[i as usize] != quote {
                    let c = bytes[i as usize];
                    if c == b'\\' && i + 1 < len {
                        let esc = bytes[i as usize + 1];
                        match esc {
                            b'n' => buf.push(b'\n'),
                            b'r' => buf.push(b'\r'),
                            b't' => buf.push(b'\t'),
                            b'b' => buf.push(0x08),
                            b'f' => buf.push(0x0c),
                            b'v' => buf.push(0x0b),
                            b'0' => buf.push(0),
                            b'\\' => buf.push(b'\\'),
                            b'\'' => buf.push(b'\''),
                            b'"' => buf.push(b'"'),
                            b'`' => buf.push(b'`'),
                            other => buf.push(other),
                        }
                        i += 2;
                        continue;
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
                    "null" => Token::Null,
                    _ => Token::Ident(name.to_string()),
                };
                emit(&mut out, token, start, i);
            }
            b if b.is_ascii_digit() => {
                // 0x... hex literal — TS / JS standard. Parse as u64 and
                // cast to f64; values up to 2^53 round-trip exactly,
                // which covers every realistic bitwise / mask use.
                if b == b'0'
                    && peek(bytes, i + 1).is_some_and(|c| c == b'x' || c == b'X')
                {
                    i += 2; // skip "0x"
                    let hex_start = i;
                    while i < len && bytes[i as usize].is_ascii_hexdigit() {
                        i += 1;
                    }
                    if i == hex_start {
                        return Err(format!("invalid hex literal at {start}"));
                    }
                    let s = std::str::from_utf8(&bytes[hex_start as usize..i as usize])
                        .expect("ascii hex digits are valid utf-8");
                    let n: u64 = u64::from_str_radix(s, 16)
                        .map_err(|_| format!("invalid hex number at {start}"))?;
                    emit(&mut out, Token::Number(n as f64), start, i);
                    continue;
                }
                while i < len && bytes[i as usize].is_ascii_digit() {
                    i += 1;
                }
                if peek(bytes, i) == Some(b'.')
                    && peek(bytes, i + 1).is_some_and(|c| c.is_ascii_digit())
                {
                    i += 1;
                    while i < len && bytes[i as usize].is_ascii_digit() {
                        i += 1;
                    }
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
                let s = std::str::from_utf8(&bytes[start as usize..i as usize])
                    .expect("ascii digits are valid utf-8");
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

fn emit(out: &mut Vec<Spanned>, token: Token, start: u32, end: u32) {
    out.push(Spanned {
        token,
        span: Span { start, end },
    });
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
