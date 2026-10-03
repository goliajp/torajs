fn survivor(people: u64, k: u64) -> u64 {
    let mut r: u64 = 0;
    let mut i: u64 = 2;
    while i <= people {
        r = (r + k) % i;
        i += 1;
    }
    r
}

fn main() {
    let mut args = std::env::args().skip(1);
    let limit: u64 = args.next().unwrap().parse().unwrap();
    let people: u64 = args.next().unwrap().parse().unwrap();
    let mut total: u64 = 0;
    for k in 1..=limit {
        total += survivor(people, k);
    }
    println!("{}", total);
}
