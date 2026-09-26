fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for i in 0..n {
        let s = i.to_string();
        total = total + s.len() as i64;
    }
    println!("{}", total);
}
