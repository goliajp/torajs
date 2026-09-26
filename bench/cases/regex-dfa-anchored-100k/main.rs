// hand-written matcher for /^\w+/, not a regex engine
fn find(s: &[u8]) -> Option<(usize, usize)> {
    let mut end = 0;
    while end < s.len() && (s[end].is_ascii_alphanumeric() || s[end] == b'_') {
        end += 1;
    }
    if end == 0 {
        None
    } else {
        Some((0, end))
    }
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for i in 0..n {
        let s = format!("hello world {}", i);
        if let Some((a, b)) = find(s.as_bytes()) {
            let m0 = s[a..b].to_string();
            total += m0.len() as i64;
        }
    }
    println!("{}", total);
}
