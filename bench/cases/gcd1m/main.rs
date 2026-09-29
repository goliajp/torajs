fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn main() {
    let mut args = std::env::args().skip(1);
    let limit: u64 = args.next().unwrap().parse().unwrap();
    let mut total: u64 = 0;
    let target: u64 = args.next().unwrap().parse().unwrap();
    for i in 1..=limit {
        total += gcd(i, target);
    }
    println!("{}", total);
}
