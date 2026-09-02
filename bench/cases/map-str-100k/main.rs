use std::collections::HashMap;

fn main() {
    let mut m: HashMap<String, i64> = HashMap::new();
    let n: i64 = 100_000;
    for i in 0..n {
        let j = i % 4096;
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
