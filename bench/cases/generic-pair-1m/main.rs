struct Pair<A, B> {
    fst: A,
    snd: B,
}

#[inline(never)]
fn loop_sum(n: i64) -> i64 {
    let mut sum: i64 = 0;
    for i in 0..n {
        // Box explicitly so the comparison stays apples-to-apples with
        // torajs's Pair (which heap-allocates each instance via
        // __torajs_obj_alloc). Without Box, rustc keeps the struct on
        // the stack and the loop becomes a pure-arithmetic kernel.
        let p: Box<Pair<i64, i64>> = Box::new(Pair { fst: i, snd: i + 1 });
        sum = sum + p.fst + p.snd;
    }
    sum
}

fn main() {
    println!("{}", loop_sum(1_000_000));
}
