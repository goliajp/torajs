use std::hint::black_box;

fn main() {
    let mut args = std::env::args().skip(1);
    let count: i64 = args.next().unwrap().parse().unwrap();
    let mut xs: Vec<i64> = Vec::new();
    let mut i: i64 = 0;
    while i < count {
        if i % 2 == 0 {
            xs.push(i);
        } else {
            xs.push(-i);
        }
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
