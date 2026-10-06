fn main() {
    let prefix = "  Hello42 word  ";
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    let tail = args.next().unwrap();
    let mut ring: Vec<String> = vec![String::new(); 8];
    let mut total: i64 = 0;
    for i in 0..n {
        // prefix + tail builds a fresh String of exactly the combined length
        let s = [prefix, tail.as_str()].concat();
        total += s.as_bytes()[(i & 15) as usize] as i64;
        ring[(i & 7) as usize] = s;
    }
    println!("{}", total);
    println!("{}", ring[((n - 1) & 7) as usize]);
}
