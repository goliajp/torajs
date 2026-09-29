struct Fail {
    code: i64,
}

fn descend(d: i64, v: i64, every: i64, finals: &mut i64) -> Result<i64, Box<Fail>> {
    if d == 0 {
        if v % every == 0 {
            return Err(Box::new(Fail { code: v }));
        }
        return Ok(v);
    }
    let r = descend(d - 1, v + 1, every, finals);
    *finals += 1;
    Ok(r? + 1)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let depth: i64 = args.next().unwrap().parse().unwrap();
    let every: i64 = args.next().unwrap().parse().unwrap();

    let mut finals: i64 = 0;
    let mut total: i64 = 0;
    for i in 0..n {
        match descend(depth, i, every, &mut finals) {
            Ok(v) => total += v,
            Err(e) => total += e.code,
        }
    }
    println!("{} {}", total, finals);
}
