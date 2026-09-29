fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let pool: i64 = args.next().unwrap().parse().unwrap();
    // each element is a heap string, as 'item' + j is in main.ts
    let mut arr: Vec<String> = Vec::new();
    for j in 0..pool {
        arr.push(format!("item{}", j));
    }
    let mut total: i64 = 0;
    for i in 0..n {
        let s = &arr[(i % pool) as usize];
        total = total + s.len() as i64;
    }
    println!("{}", total);
}
