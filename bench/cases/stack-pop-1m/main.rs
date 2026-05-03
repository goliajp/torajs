fn main() {
    let mut xs: Vec<i64> = Vec::new();
    let n: i64 = 1_000_000;
    for i in 0..n {
        xs.push(i);
    }
    let mut total: i64 = 0;
    while let Some(v) = xs.pop() {
        total += v;
    }
    println!("{}", total);
}
