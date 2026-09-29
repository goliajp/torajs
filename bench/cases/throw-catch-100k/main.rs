use std::panic;

fn trial(i: i64) -> i64 {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> i64 {
        panic::panic_any(i)
    }));
    *result.unwrap_err().downcast_ref::<i64>().unwrap()
}

fn main() {
    // Silence the default panic hook so the bench harness's stderr stays
    // clean — every trial intentionally panics.
    panic::set_hook(Box::new(|_| {}));
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let mut total: i64 = 0;
    for i in 0..n {
        total = total + trial(i);
    }
    println!("{}", total);
}
