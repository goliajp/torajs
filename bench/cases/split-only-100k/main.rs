fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for _ in 0..n {
        let parts: Vec<&str> = "3 4 + 2 * 5 +".split(' ').collect();
        total += parts.len() as i64;
    }
    println!("{}", total);
}
