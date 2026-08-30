use std::hint::black_box;

fn main() {
    let mut src: Vec<i64> = Vec::new();
    let mut i: i64 = 0;
    while i < 10_000_000 {
        src.push(i);
        i += 1;
    }
    let mut dst: Vec<i64> = Vec::new();
    let mut j: usize = 0;
    while j < src.len() {
        dst.push(black_box(src[j]));
        j += 1;
    }
    println!("{}", dst.len() as i64 + dst[9999999]);
}
