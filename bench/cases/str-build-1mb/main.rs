fn next(s: i64) -> i64 {
    let t = ((s * 48271) as i32 & 0x7fffffff) as i64;
    if t == 0 {
        1
    } else {
        t
    }
}

fn main() {
    let words: Vec<String> = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi", "omicron", "pi",
    ]
    .iter()
    .map(|w| w.to_string())
    .collect();

    let mut args = std::env::args().skip(1);
    let target: usize = args.next().unwrap().parse().unwrap();
    let rounds: i64 = args.next().unwrap().parse().unwrap();
    let mut seed: i64 = args.next().unwrap().parse().unwrap();

    let mut total: i64 = 0;
    for _ in 0..rounds {
        let mut s = String::new();
        while s.len() < target {
            seed = next(seed);
            s.push_str(&words[(seed & 15) as usize]);
            s.push(' ');
        }
        total += s.len() as i64 + s.as_bytes()[s.len() >> 1] as i64;
    }
    println!("{}", total);
}
