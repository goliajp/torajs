fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    let mut i: u64 = 2;
    while i * i <= n {
        if n % i == 0 {
            return false;
        }
        i += 1;
    }
    true
}

fn main() {
    let mut args = std::env::args().skip(1);
    let limit: u64 = args.next().unwrap().parse().unwrap();
    let mut count: u64 = 0;
    let mut n: u64 = 0;
    while n < limit {
        if is_prime(n) {
            count += 1;
        }
        n += 1;
    }
    println!("{}", count);
}
