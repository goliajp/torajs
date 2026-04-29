use std::hint::black_box;
use std::rc::Rc;

struct Pair {
    a: i64,
    b: i64,
}

fn consume(x: Rc<Pair>) -> i64 {
    black_box(x.a)
}

fn main() {
    let u: Rc<Pair> = Rc::new(Pair { a: 1, b: 2 });
    let mut acc: i64 = 0;
    for _ in 0..10_000_000 {
        acc += consume(Rc::clone(&u));
    }
    println!("{}", acc);
}
