use std::fmt::Write;
use std::hint::black_box;

struct Record {
    id: i64,
    name: &'static str,
    score: i64,
    active: bool,
}

fn make_record(i: i64) -> Box<Record> {
    Box::new(Record {
        id: i,
        name: "row",
        score: i * 7,
        active: (i & 1) == 0,
    })
}

// QuoteJSONString: escape '"', '\\' and control chars
fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

// the key order and value kinds are fixed by the struct; JSON.stringify
// discovers both at run time
fn stringify(r: &Record) -> String {
    let mut out = String::new();
    out.push('{');
    quote(&mut out, "id");
    out.push(':');
    write!(out, "{}", r.id).unwrap();
    out.push(',');
    quote(&mut out, "name");
    out.push(':');
    quote(&mut out, r.name);
    out.push(',');
    quote(&mut out, "score");
    out.push(':');
    write!(out, "{}", r.score).unwrap();
    out.push(',');
    quote(&mut out, "active");
    out.push(':');
    out.push_str(if r.active { "true" } else { "false" });
    out.push('}');
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        // black_box keeps the record allocation, which LLVM would otherwise elide
        let r = black_box(make_record(i));
        let s = stringify(&r);
        total = total + s.len() as i64;
    }
    println!("{}", total);
}
