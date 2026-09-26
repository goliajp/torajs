fn build(n: usize, seed: i64) -> Vec<i64> {
    let mut xs: Vec<i64> = Vec::new();
    let mut s = seed as i32;
    for _ in 0..n {
        // (s * 48271) | 0: the product stays below 2^53, then wraps to int32
        s = ((s as i64 * 48271) as i32) & 0x7fffffff;
        if s == 0 {
            s = 1;
        }
        xs.push(s as i64);
    }
    xs
}

fn main() {
    let mut checksum: i64 = 0;
    let passes: i64 = 100;
    for p in 0..passes {
        let mut xs = build(1000, p + 1);
        // stable comparison sort driven by the (a, b) => a - b comparator
        xs.sort_by(|a, b| (a - b).cmp(&0));
        checksum = checksum + xs[0] + xs[999];
    }
    println!("{}", checksum);
}
