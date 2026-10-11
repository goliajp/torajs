//! A minimal JSON reader, enough for the Wycheproof vector files.

use std::collections::BTreeMap;

#[derive(Debug)]
pub enum Json {
    Null,
    True,
    False,
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub fn parse(text: &str) -> Json {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        let v = p.value();
        p.ws();
        assert_eq!(p.i, p.s.len(), "trailing data after JSON value");
        v
    }

    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(m) => m.get(key).unwrap_or_else(|| panic!("missing key {key}")),
            _ => panic!("not an object"),
        }
    }

    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            _ => panic!("not a string"),
        }
    }

    pub fn num(&self) -> f64 {
        match self {
            Json::Num(n) => *n,
            _ => panic!("not a number"),
        }
    }

    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => panic!("not an array"),
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.s[self.i], c, "expected {:?} at {}", c as char, self.i);
        self.i += 1;
    }

    fn peek(&mut self) -> u8 {
        self.ws();
        self.s[self.i]
    }

    fn lit(&mut self, word: &str, v: Json) -> Json {
        assert!(self.s[self.i..].starts_with(word.as_bytes()), "bad literal");
        self.i += word.len();
        v
    }

    fn value(&mut self) -> Json {
        match self.peek() {
            b'{' => {
                self.eat(b'{');
                let mut m = BTreeMap::new();
                if self.peek() != b'}' {
                    loop {
                        let k = self.string();
                        self.eat(b':');
                        m.insert(k, self.value());
                        if self.peek() == b',' {
                            self.eat(b',');
                        } else {
                            break;
                        }
                    }
                }
                self.eat(b'}');
                Json::Obj(m)
            }
            b'[' => {
                self.eat(b'[');
                let mut a = Vec::new();
                if self.peek() != b']' {
                    loop {
                        a.push(self.value());
                        if self.peek() == b',' {
                            self.eat(b',');
                        } else {
                            break;
                        }
                    }
                }
                self.eat(b']');
                Json::Arr(a)
            }
            b'"' => Json::Str(self.string()),
            b't' => self.lit("true", Json::True),
            b'f' => self.lit("false", Json::False),
            b'n' => self.lit("null", Json::Null),
            _ => {
                let start = self.i;
                while self.i < self.s.len() && b"+-.eE0123456789".contains(&self.s[self.i]) {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).unwrap();
                Json::Num(t.parse().unwrap_or_else(|_| panic!("bad number {t:?}")))
            }
        }
    }

    fn string(&mut self) -> String {
        self.eat(b'"');
        let mut out = Vec::new();
        loop {
            let c = self.s[self.i];
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.s[self.i];
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
                            let cp = u32::from_str_radix(h, 16).unwrap();
                            self.i += 4;
                            let ch = char::from_u32(cp).expect("surrogate escapes unsupported");
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => panic!("bad escape"),
                    }
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).unwrap()
    }
}

pub fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
