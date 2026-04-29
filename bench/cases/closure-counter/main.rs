use std::hint::black_box;

#[inline(never)]
fn loop_sum(xs: &[i64], f: &dyn Fn(i64) -> i64) -> i64 {
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
    let offset: i64 = black_box(2);
    // Box::new(move |x| x + offset) is the rust equivalent of a
    // capturing arrow with heap-allocated env; the &dyn Fn vtable
    // dispatch matches torajs's load-fn_ptr-from-env + indirect call.
    // #[inline(never)] on loop_sum + black_box on the &dyn Fn together
    // prevent rustc from devirtualizing the call back to a direct fn
    // pointer (without these, LLVM monomorphizes through the only
    // concrete closure type and the indirect-call cost vanishes).
    let f: Box<dyn Fn(i64) -> i64> = Box::new(move |x| x + offset);
    let f_ref: &dyn Fn(i64) -> i64 = black_box(&*f);
    println!("{}", loop_sum(&xs, f_ref));
}
