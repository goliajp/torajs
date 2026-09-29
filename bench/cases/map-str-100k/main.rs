use std::collections::HashMap;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut m: HashMap<String, i64> = HashMap::new();
    let n: i64 = args.next().unwrap().parse().unwrap();
    let modulus: i64 = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        let j = i % modulus;
        let key = if i % 2 == 0 {
            format!("key{}", j)
        } else {
            format!("ключ{}", j)
        };
        *m.entry(key).or_insert(0) += i;
    }
    let mut total: i64 = 0;
    for v in m.values() {
        total += v;
    }
    println!("{} {}", m.len(), total);
}
