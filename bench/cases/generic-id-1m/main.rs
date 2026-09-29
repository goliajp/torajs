fn id<T>(x: T) -> T {
    x
}

#[inline(never)]
fn loop_sum(xs: &[i64]) -> i64 {
    let mut sum: i64 = 0;
    for &x in xs {
        sum = sum + id::<i64>(x);
    }
    sum
}

fn main() {
    let mut args = std::env::args().skip(1);
    let count: i64 = args.next().unwrap().parse().unwrap();
    let mut xs: Vec<i64> = Vec::new();
    for i in 0..count {
        xs.push(i);
    }
    println!("{}", loop_sum(&xs));
}
