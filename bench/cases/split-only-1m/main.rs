fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    for _ in 0..n {
        let parts: Vec<&str> = "3 4 + 2 * 5 +".split(' ').collect();
        total += parts.len() as i64;
    }
    println!("{}", total);
}
