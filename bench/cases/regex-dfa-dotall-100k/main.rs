// hand-written matcher for /a.+c/s, not a regex engine
fn find(s: &[u8]) -> Option<(usize, usize)> {
    // greedy `.+` backs off to the last `c` that leaves at least one byte after the `a`
    for a in 0..s.len() {
        if s[a] != b'a' {
            continue;
        }
        let mut c = s.len();
        while c > a + 2 {
            c -= 1;
            if s[c] == b'c' {
                return Some((a, c + 1));
            }
        }
    }
    None
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for i in 0..n {
        let s = format!("pre a\nmiddle\nc post {}", i);
        if let Some((a, b)) = find(s.as_bytes()) {
            let m0 = s[a..b].to_string();
            total += m0.len() as i64;
        }
    }
    println!("{}", total);
}
