use std::hint::black_box;

fn add1(x: i64) -> i64 {
    x + 1
}

fn reduce(xs: &[i64], f: fn(i64) -> i64) -> i64 {
    let mut sum: i64 = 0;
    for &x in xs {
        sum = sum + f(x);
    }
    sum
}

fn main() {
    let mut xs: Vec<i64> = Vec::new();
    for i in 0..10_000_000 {
        xs.push(i);
    }
    // `black_box(add1)` opaques the fn pointer so the rust optimizer
    // can't devirtualize the indirect call. Matches torajs (AOT)
    // which always emits a real CallIndirect — no devirt yet.
    println!("{}", reduce(&xs, black_box(add1 as fn(i64) -> i64)));
}
