fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let input = args.next().unwrap();
    let sep = args.next().unwrap();
    let mut total: i64 = 0;
    for _ in 0..n {
        let parts: Vec<&str> = input.split(sep.as_str()).collect();
        total += parts.len() as i64;
    }
    println!("{}", total);
}
