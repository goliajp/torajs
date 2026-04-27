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
    let mut total: u64 = 0;
    for i in 0..10_000_000_u64 {
        total += popcount(i);
    }
    println!("{}", total);
}
