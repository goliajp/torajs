use std::collections::HashMap;

struct Rec {
    key: String,
    hits: Option<i64>,
    n: i64,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let mut recs: Vec<Rec> = Vec::new();
    for i in 0..64i64 {
        if i % 3 == 0 {
            recs.push(Rec { key: format!("k{}", i), hits: None, n: i });
        } else {
            recs.push(Rec { key: format!("k{}", i), hits: Some(i), n: i });
        }
    }
    // the keys borrow the records' strings: a lookup allocates nothing
    let mut counts: HashMap<&str, i64> = HashMap::new();
    let mut total: i64 = 0;
    for i in 0..n {
        let r = &recs[(i & 63) as usize];
        *counts.entry(r.key.as_str()).or_insert(0) += 1;
        total = total + r.hits.unwrap_or(1) + r.n;
    }
    let mut sum: i64 = 0;
    for v in counts.values() {
        sum += v;
    }
    println!("{} {} {}", total, sum, counts.len());
}
