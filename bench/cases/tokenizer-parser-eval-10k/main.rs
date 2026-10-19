fn next(s: i64) -> i64 {
    let t = (s * 48271) % 2147483647;
    if t == 0 {
        1
    } else {
        t
    }
}

fn gen(depth: i64, seed: &mut i64) -> String {
    *seed = next(*seed);
    let r = *seed % 6;
    if depth == 0 || r == 0 {
        *seed = next(*seed);
        if (*seed & 1) == 0 {
            return (*seed % 1000).to_string();
        }
        return ((97 + ((*seed >> 1) & 7)) as u8 as char).to_string();
    }
    if r == 1 {
        let mut s = String::from("-");
        s.push_str(&gen(depth - 1, seed));
        return s;
    }
    if r == 2 {
        let mut s = String::from("(");
        s.push_str(&gen(depth - 1, seed));
        s.push(')');
        return s;
    }
    let mut s = gen(depth - 1, seed);
    *seed = next(*seed);
    let op = *seed % 3;
    let right = gen(depth - 1, seed);
    if op == 0 {
        s.push_str(" + ");
    } else if op == 1 {
        s.push_str(" - ");
    } else {
        s.push_str(" * ");
    }
    s.push_str(&right);
    s
}

const NUM: u8 = 0;
const IDENT: u8 = 1;
const PLUS: u8 = 2;
const MINUS: u8 = 3;
const STAR: u8 = 4;
const LPAREN: u8 = 5;
const RPAREN: u8 = 6;
const END: u8 = 7;

#[derive(Clone, Copy)]
struct Token {
    kind: u8,
    value: i32,
}

fn tokenize(src: &str) -> Vec<Token> {
    let src = src.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if c == 32 {
            i += 1;
        } else if c >= 48 && c <= 57 {
            let mut v: i32 = 0;
            while i < src.len() {
                let d = src[i];
                if d < 48 || d > 57 {
                    break;
                }
                v = v * 10 + (d - 48) as i32;
                i += 1;
            }
            out.push(Token {
                kind: NUM,
                value: v,
            });
        } else if c >= 97 && c <= 122 {
            out.push(Token {
                kind: IDENT,
                value: (c - 97) as i32,
            });
            i += 1;
        } else {
            let mut kind = RPAREN;
            if c == 43 {
                kind = PLUS;
            } else if c == 45 {
                kind = MINUS;
            } else if c == 42 {
                kind = STAR;
            } else if c == 40 {
                kind = LPAREN;
            }
            out.push(Token { kind, value: 0 });
            i += 1;
        }
    }
    out.push(Token {
        kind: END,
        value: 0,
    });
    out
}

const NEG: u8 = 8;

struct Expr {
    kind: u8,
    value: i32,
    left: Option<Box<Expr>>,
    right: Option<Box<Expr>>,
}

struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    nodes: &'a mut i64,
}

impl<'a> Parser<'a> {
    fn expr(&mut self) -> Box<Expr> {
        let mut left = self.term();
        while self.toks[self.pos].kind == PLUS || self.toks[self.pos].kind == MINUS {
            let kind = self.toks[self.pos].kind;
            self.pos += 1;
            let right = self.term();
            *self.nodes += 1;
            left = Box::new(Expr {
                kind,
                value: 0,
                left: Some(left),
                right: Some(right),
            });
        }
        left
    }

    fn term(&mut self) -> Box<Expr> {
        let mut left = self.factor();
        while self.toks[self.pos].kind == STAR {
            self.pos += 1;
            let right = self.factor();
            *self.nodes += 1;
            left = Box::new(Expr {
                kind: STAR,
                value: 0,
                left: Some(left),
                right: Some(right),
            });
        }
        left
    }

    fn factor(&mut self) -> Box<Expr> {
        let t = self.toks[self.pos];
        self.pos += 1;
        *self.nodes += 1;
        if t.kind == NUM || t.kind == IDENT {
            return Box::new(Expr {
                kind: t.kind,
                value: t.value,
                left: None,
                right: None,
            });
        }
        if t.kind == MINUS {
            let inner = self.factor();
            return Box::new(Expr {
                kind: NEG,
                value: 0,
                left: Some(inner),
                right: None,
            });
        }
        let inner = self.expr();
        self.pos += 1;
        *self.nodes -= 1;
        inner
    }
}

fn evaluate(e: &Expr, env: &[i32]) -> i32 {
    if e.kind == NUM {
        return e.value;
    }
    if e.kind == IDENT {
        return env[e.value as usize];
    }
    if e.kind == NEG {
        return 0i32.wrapping_sub(evaluate(e.left.as_ref().unwrap(), env));
    }
    let a = evaluate(e.left.as_ref().unwrap(), env);
    let b = evaluate(e.right.as_ref().unwrap(), env);
    if e.kind == PLUS {
        return a.wrapping_add(b);
    }
    if e.kind == MINUS {
        return a.wrapping_sub(b);
    }
    a.wrapping_mul(b)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let line_count: usize = args.next().unwrap().parse().unwrap();
    let depth: i64 = args.next().unwrap().parse().unwrap();
    let rounds: usize = args.next().unwrap().parse().unwrap();
    let mut seed: i64 = args.next().unwrap().parse().unwrap();

    let mut lines: Vec<String> = Vec::new();
    for _ in 0..line_count {
        lines.push(gen(depth, &mut seed));
    }

    let mut env: Vec<i32> = vec![3, 5, 7, 11, 13, 17, 19, 23];
    let mut total: i32 = 0;
    let mut tokens: i64 = 0;
    let mut nodes: i64 = 0;
    for r in 0..rounds {
        for line in &lines {
            let toks = tokenize(line);
            tokens += toks.len() as i64;
            let mut p = Parser {
                toks,
                pos: 0,
                nodes: &mut nodes,
            };
            total = total.wrapping_add(evaluate(&p.expr(), &env));
        }
        env[r & 7] = env[r & 7].wrapping_add(total);
    }
    println!("{} {} {}", total, tokens, nodes);
}
