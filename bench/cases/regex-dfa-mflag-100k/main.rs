// hand-written matcher for /^line/m, not a regex engine
fn line_start(s: &[u8], a: usize) -> bool {
    // js line terminators: \n, \r, U+2028, U+2029
    a == 0
        || s[a - 1] == b'\n'
        || s[a - 1] == b'\r'
        || (a >= 3 && s[a - 3] == 0xE2 && s[a - 2] == 0x80 && (s[a - 1] == 0xA8 || s[a - 1] == 0xA9))
}

fn find(s: &[u8]) -> Option<(usize, usize)> {
    let pat = b"line";
    if s.len() < pat.len() {
        return None;
    }
    for a in 0..=s.len() - pat.len() {
        if line_start(s, a) && &s[a..a + pat.len()] == pat {
            return Some((a, a + pat.len()));
        }
    }
    None
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        let s = format!("pre\nline-text\nend {}", i);
        if let Some((a, b)) = find(s.as_bytes()) {
            let m0 = s[a..b].to_string();
            total += m0.len() as i64;
        }
    }
    println!("{}", total);
}
