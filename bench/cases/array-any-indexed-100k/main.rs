fn main() {
    // each element is a heap string, as 'item' + j is in main.ts
    let mut arr: Vec<String> = Vec::new();
    for j in 0..100 {
        arr.push(format!("item{}", j));
    }
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for i in 0..n {
        let s = &arr[(i % 100) as usize];
        total = total + s.len() as i64;
    }
    println!("{}", total);
}
