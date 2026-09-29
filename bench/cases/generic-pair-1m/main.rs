struct Pair<A, B> {
    fst: A,
    snd: B,
}

fn loop_sum(n: i64) -> i64 {
    let mut sum: i64 = 0;
    for i in 0..n {
        let p: Pair<i64, i64> = Pair { fst: i, snd: i + 1 };
        sum = sum + p.fst + p.snd;
    }
    sum
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    println!("{}", loop_sum(n));
}
