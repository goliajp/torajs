// hand-written matcher for /hello/i, not a regex engine; without the u flag js folds
// no non-ascii character onto an ascii one, so ascii case folding is exact here
fn find(s: &[u8]) -> Option<(usize, usize)> {
    let pat = b"hello";
    if s.len() < pat.len() {
        return None;
    }
    for a in 0..=s.len() - pat.len() {
        if s[a..a + pat.len()].eq_ignore_ascii_case(pat) {
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
        let s = format!("before HELLO world {}", i);
        if let Some((a, b)) = find(s.as_bytes()) {
            let m0 = s[a..b].to_string();
            total += m0.len() as i64;
        }
    }
    println!("{}", total);
}
