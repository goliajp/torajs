fn divisor_sum(n: u64) -> u64 {
    let mut s: u64 = 0;
    let mut d: u64 = 1;
    while d * d <= n {
        if n % d == 0 {
            s += d;
        }
        d += 1;
    }
    s
}

fn main() {
    let mut args = std::env::args().skip(1);
    let limit: u64 = args.next().unwrap().parse().unwrap();
    let mut total: u64 = 0;
    for n in 1..=limit {
        total += divisor_sum(n);
    }
    println!("{}", total);
}
