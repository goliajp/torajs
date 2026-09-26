// hand-written matcher for /[A-Za-z]+/u, not a regex engine; an ascii-only class never
// matches a utf-8 lead or continuation byte, so stepping bytes gives the u-flag matches
fn find(s: &[u8]) -> Option<(usize, usize)> {
    let a = s.iter().position(|b| b.is_ascii_alphabetic())?;
    let mut end = a + 1;
    while end < s.len() && s[end].is_ascii_alphabetic() {
        end += 1;
    }
    Some((a, end))
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for i in 0..n {
        let s = format!("  Hello42 word  {}", i);
        if let Some((a, b)) = find(s.as_bytes()) {
            let m0 = s[a..b].to_string();
            total += m0.len() as i64;
        }
    }
    println!("{}", total);
}
