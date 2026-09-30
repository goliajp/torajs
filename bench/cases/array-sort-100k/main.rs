fn build(n: usize, seed: i64) -> Vec<i64> {
    let mut xs: Vec<i64> = Vec::new();
    let mut s = seed as i32;
    for _ in 0..n {
        // the minimal standard generator; the product stays below 2^47
        s = ((s as i64 * 48271) % 2147483647) as i32;
        if s == 0 {
            s = 1;
        }
        xs.push(s as i64);
    }
    xs
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut checksum: i64 = 0;
    let passes: i64 = args.next().unwrap().parse().unwrap();
    let len: usize = args.next().unwrap().parse().unwrap();
    for p in 0..passes {
        let mut xs = build(len, p + 1);
        // stable comparison sort driven by the (a, b) => a - b comparator
        xs.sort_by(|a, b| (a - b).cmp(&0));
        checksum = checksum + xs[0] + xs[len - 1];
    }
    println!("{}", checksum);
}
