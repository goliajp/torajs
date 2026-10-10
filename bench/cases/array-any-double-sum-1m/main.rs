fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let pool: i64 = args.next().unwrap().parse().unwrap();
    let mut arr: Vec<f64> = Vec::new();
    for j in 0..pool {
        arr.push(j as f64 + 0.5);
    }
    let mut total: f64 = 0.0;
    for i in 0..n {
        total = total + arr[(i % pool) as usize];
    }
    println!("{}", total);
}
