fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn main() {
    let mut total: u64 = 0;
    let target: u64 = 1234567;
    for i in 1..=1000000_u64 {
        total += gcd(i, target);
    }
    println!("{}", total);
}
