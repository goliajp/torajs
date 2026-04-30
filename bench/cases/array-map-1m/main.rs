use std::hint::black_box;

#[inline(never)]
fn loop_sum(n: i64, k: i64) -> i64 {
    let mut xs: Vec<i64> = Vec::new();
    for i in 0..n {
        xs.push(i);
    }
    // Force the closure to be a real boxed callable so the indirect
    // call cost matches torajs's Closure path. Without black_box +
    // explicit Fn dyn, rustc inlines + vectorizes the loop into pure
    // arithmetic and the comparison is no longer apples-to-apples.
    let f: Box<dyn Fn(i64) -> i64> = Box::new(move |x| x + k);
    let f_ref: &dyn Fn(i64) -> i64 = black_box(&*f);
    let ys: Vec<i64> = xs.iter().map(|&x| f_ref(x)).collect();
    let mut sum: i64 = 0;
    for &y in &ys {
        sum = sum + y;
    }
    sum
}

fn main() {
    println!("{}", loop_sum(10_000_000, 2));
}
