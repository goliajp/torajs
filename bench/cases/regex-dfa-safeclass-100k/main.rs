// hand-written matcher for /[A-Za-z0-9]+/, not a regex engine
fn find(s: &[u8]) -> Option<(usize, usize)> {
    let a = s.iter().position(|b| b.is_ascii_alphanumeric())?;
    let mut end = a + 1;
    while end < s.len() && s[end].is_ascii_alphanumeric() {
        end += 1;
    }
    Some((a, end))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        let s = format!("  Hello42 world  {}", i);
        if let Some((a, b)) = find(s.as_bytes()) {
            let m0 = s[a..b].to_string();
            total += m0.len() as i64;
        }
    }
    println!("{}", total);
}
