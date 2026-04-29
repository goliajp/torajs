use std::hint::black_box;

fn main() {
    let mut xs: Vec<i64> = Vec::new();
    let mut i: i64 = 0;
    while i < 10_000_000 {
        xs.push(i);
        i += 1;
    }
    let mut sum: i64 = 0;
    let mut j: usize = 0;
    while j < xs.len() {
        sum = sum + black_box(xs[j]);
        j += 1;
    }
    println!("{}", sum);
}
