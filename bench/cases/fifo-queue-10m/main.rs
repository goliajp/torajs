use std::collections::VecDeque;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut q: VecDeque<i64> = VecDeque::new();
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    let window: usize = args.next().unwrap().parse().unwrap();
    for i in 0..n {
        q.push_back(i);
        if q.len() > window {
            total += q.pop_front().unwrap();
        }
    }
    while let Some(v) = q.pop_front() {
        total += v;
    }
    println!("{}", total);
}
