fn row_len(line: &str) -> i64 {
    let mut total: i64 = 0;
    for part in line.split(',') {
        total += part.trim().len() as i64;
    }
    total
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for _ in 0..n {
        total += row_len("  alpha , beta , gamma , delta , epsilon , zeta  ");
    }
    println!("{}", total);
}
