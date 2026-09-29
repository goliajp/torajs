fn ackermann(m: u64, n: u64) -> u64 {
    if m == 0 {
        return n + 1;
    }
    if n == 0 {
        return ackermann(m - 1, 1);
    }
    ackermann(m - 1, ackermann(m, n - 1))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let m: u64 = args.next().unwrap().parse().unwrap();
    let n: u64 = args.next().unwrap().parse().unwrap();
    println!("{}", ackermann(m, n));
}
