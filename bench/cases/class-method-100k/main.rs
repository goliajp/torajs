use std::hint::black_box;

struct Vec2 {
    x: i64,
    y: i64,
}

impl Vec2 {
    // boxed like generic-pair-1m, since new Vec2 heap-allocates in main.ts
    fn new(x: i64, y: i64) -> Box<Vec2> {
        Box::new(Vec2 { x, y })
    }

    fn norm(&self) -> i64 {
        self.x * self.x + self.y * self.y
    }

    fn scale(&self, k: i64) -> Box<Vec2> {
        Vec2::new(self.x * k, self.y * k)
    }
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for i in 0..n {
        // black_box keeps both allocations, which LLVM would otherwise elide
        let v = black_box(Vec2::new(i, i + 1));
        let s = black_box(v.scale(2));
        total = total + s.norm();
    }
    println!("{}", total);
}
