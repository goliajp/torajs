struct Opt {
    a: Option<i64>,
    b: i64,
}

fn make(i: i64) -> Opt {
    if i % 4 == 0 {
        return Opt { a: Some(i), b: i };
    }
    Opt { a: None, b: i }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let mut total: i64 = 0;
    let mut created: i64 = 0;
    for i in 0..n {
        let mut o = make(i);
        if o.a.is_none() {
            o.a = Some(2);
            created += 1;
        }
        o.b += 1;
        total = total + o.a.unwrap_or(0) + o.b;
    }
    println!("{} {}", total, created);
}
