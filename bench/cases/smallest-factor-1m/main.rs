fn smallest_factor(n: u64) -> u64 {
    let mut d: u64 = 2;
    while d * d <= n {
        if n % d == 0 {
            return d;
        }
        d += 1;
    }
    n
}

fn main() {
    let mut args = std::env::args().skip(1);
    let limit: u64 = args.next().unwrap().parse().unwrap();
    let mut total: u64 = 0;
    let mut n: u64 = 2;
    while n < limit {
        total += smallest_factor(n);
        n += 1;
    }
    println!("{}", total);
}
