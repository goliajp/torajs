fn steps(mut n: u64) -> u64 {
    let mut count: u64 = 0;
    while n != 1 {
        if n & 1 == 0 {
            n >>= 1;
        } else {
            n = 3 * n + 1;
        }
        count += 1;
    }
    count
}

fn main() {
    let mut args = std::env::args().skip(1);
    let limit: u64 = args.next().unwrap().parse().unwrap();
    let mut max: u64 = 0;
    let mut i: u64 = 1;
    while i <= limit {
        let s = steps(i);
        if s > max {
            max = s;
        }
        i += 1;
    }
    println!("{}", max);
}
