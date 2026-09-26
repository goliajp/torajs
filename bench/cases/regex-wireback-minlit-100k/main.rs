// hand-written matcher for /x/, not a regex engine
fn find(s: &[u8]) -> Option<(usize, usize)> {
    let a = s.iter().position(|&b| b == b'x')?;
    Some((a, a + 1))
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for _ in 0..n {
        let s = "x";
        if let Some((a, b)) = find(s.as_bytes()) {
            // the match array's group 0 is a fresh string in js too
            let _m0 = s[a..b].to_string();
            total += 1;
        }
    }
    println!("{}", total);
}
