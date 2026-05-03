fn rebuild(line: &str) -> i64 {
    let mut total: i64 = 0;
    for part in line.split(',') {
        let s = format!("{}|", part);
        total += s.as_bytes()[0] as i64;
    }
    total
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for _ in 0..n {
        total += rebuild("alpha,beta,gamma,delta,epsilon,zeta");
    }
    println!("{}", total);
}
