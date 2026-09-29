fn main() {
    let mut args = std::env::args().skip(1);
    let mut xs: Vec<i64> = Vec::new();
    let n: i64 = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        xs.push(i);
    }
    let mut total: i64 = 0;
    while let Some(v) = xs.pop() {
        total += v;
    }
    println!("{}", total);
}
