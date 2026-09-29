// hand-written matcher for /\bword\b/, not a regex engine
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn find(s: &[u8]) -> Option<(usize, usize)> {
    let pat = b"word";
    if s.len() < pat.len() {
        return None;
    }
    for a in 0..=s.len() - pat.len() {
        let b = a + pat.len();
        if &s[a..b] == pat && (a == 0 || !is_word(s[a - 1])) && (b == s.len() || !is_word(s[b])) {
            return Some((a, b));
        }
    }
    None
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        let s = format!("a word b xword wordy {}", i);
        if let Some((a, b)) = find(s.as_bytes()) {
            let m0 = s[a..b].to_string();
            total += m0.len() as i64;
        }
    }
    println!("{}", total);
}
