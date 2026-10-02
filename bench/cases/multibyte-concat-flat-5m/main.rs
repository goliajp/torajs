// js strings are UTF-16 and .length counts code units, so the string is a
// Vec<u16> here; a UTF-8 String would copy 3 bytes per char instead of 2 and
// need a scan to get the code-unit length
fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let piece: Vec<u16> = "中文测试".encode_utf16().collect();
    let n: i64 = args.next().unwrap().parse().unwrap();
    let pieces: i32 = args.next().unwrap().parse().unwrap();
    for _ in 0..n {
        let mut acc: Vec<u16> = Vec::new();
        for _ in 0..pieces {
            // acc = acc + piece on a uniquely owned acc appends in place
            acc.extend_from_slice(&piece);
        }
        total = total + acc.len() as i64 + acc[acc.len() - 1] as i64;
    }
    println!("{}", total);
}
