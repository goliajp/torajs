use std::hint::black_box;

fn main() {
    let prefix = "  Hello42 word  ";
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for _ in 0..n {
        // prefix + 'x' builds a fresh string of exactly the combined length;
        // without black_box LLVM elides the allocation and folds the loop
        let s = black_box([prefix, "x"].concat());
        total = total + s.len() as i64;
    }
    println!("{}", total);
}
