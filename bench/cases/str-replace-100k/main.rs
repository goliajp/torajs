// hand-coded matcher for /a(b+)c/g: b+ is greedy and backtracking can only
// hand a 'b' to c, which never matches, so one forward scan is exact
fn replace_abc(s: &str, with: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        if b[i] == b'a' {
            let mut j = i + 1;
            while j < b.len() && b[j] == b'b' {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'c' {
                out.push_str(&s[last..i]);
                out.push_str(with);
                i = j + 1;
                last = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&s[last..]);
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    for _ in 0..n {
        let s = "abbc xxx abc yyy abbbbc";
        let r = replace_abc(s, "XY");
        total = total + r.len() as i64;
    }
    println!("{}", total);
}
