fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        let s = i.to_string();
        total = total + s.len() as i64;
    }
    println!("{}", total);
}
