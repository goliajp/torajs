fn popcount(x: u64) -> u64 {
    let mut n = x;
    let mut count = 0;
    while n != 0 {
        n &= n - 1;
        count += 1;
    }
    count
}

fn main() {
    let mut args = std::env::args().skip(1);
    let limit: u64 = args.next().unwrap().parse().unwrap();
    let mut total: u64 = 0;
    for i in 0..limit {
        total += popcount(i);
    }
    println!("{}", total);
}
