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
    Eq,
    EqEqEq,
    BangEqEq,
    FatArrow,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Eof,
}

#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone)]
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
            b'.' => emit(&mut out, Token::Dot, start, advance(&mut i)),
            b',' => emit(&mut out, Token::Comma, start, advance(&mut i)),
            b':' => emit(&mut out, Token::Colon, start, advance(&mut i)),
            b';' => emit(&mut out, Token::Semi, start, advance(&mut i)),
            b'(' => emit(&mut out, Token::LParen, start, advance(&mut i)),
            b')' => emit(&mut out, Token::RParen, start, advance(&mut i)),
            b'{' => emit(&mut out, Token::LBrace, start, advance(&mut i)),
            b'}' => emit(&mut out, Token::RBrace, start, advance(&mut i)),
            b'[' => emit(&mut out, Token::LBracket, start, advance(&mut i)),
            b']' => emit(&mut out, Token::RBracket, start, advance(&mut i)),
            b'+' => emit(&mut out, Token::Plus, start, advance(&mut i)),
            b'-' => emit(&mut out, Token::Minus, start, advance(&mut i)),
            b'*' => emit(&mut out, Token::Star, start, advance(&mut i)),
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
                    _ => emit(&mut out, Token::Slash, start, advance(&mut i)),
                }
            }
            b'%' => emit(&mut out, Token::Percent, start, advance(&mut i)),
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
            b'"' => {
                i += 1;
                let str_start = i;
                while i < len && bytes[i as usize] != b'"' {
                    // P0 has no escapes; defer
                    i += 1;
                }
                if i >= len {
                    return Err(format!("unterminated string starting at {start}"));
                }
                let value = std::str::from_utf8(&bytes[str_start as usize..i as usize])
                    .map_err(|_| format!("invalid utf-8 in string at {start}"))?
                    .to_string();
                i += 1; // consume closing quote
                emit(&mut out, Token::String(value), start, i);
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
                    _ => Token::Ident(name.to_string()),
                };
                emit(&mut out, token, start, i);
            }
            b if b.is_ascii_digit() => {
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
