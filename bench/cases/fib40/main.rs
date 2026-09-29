fn fib(n: u64) -> u64 {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: u64 = args.next().unwrap().parse().unwrap();
    println!("{}", fib(n));
}
