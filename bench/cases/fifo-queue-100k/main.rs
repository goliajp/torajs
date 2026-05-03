use std::collections::VecDeque;

fn main() {
    let mut q: VecDeque<i64> = VecDeque::new();
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for i in 0..n {
        q.push_back(i);
        if q.len() > 16 {
            total += q.pop_front().unwrap();
        }
    }
    while let Some(v) = q.pop_front() {
        total += v;
    }
    println!("{}", total);
}
