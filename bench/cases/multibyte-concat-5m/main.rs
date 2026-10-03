// js strings are UTF-16 and .length counts code units, so the string is a
// Vec<u16> here; a UTF-8 String would copy 3 bytes per char instead of 2 and
// need a scan to get the code-unit length
//
// "中文测试" as UTF-16 code units, a constant as the literal is in main.ts
// and main.c, so each append is a fixed 8-byte copy, not a memcpy call
const PIECE: [u16; 4] = [0x4e2d, 0x6587, 0x6d4b, 0x8bd5];

fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let n: i64 = args.next().unwrap().parse().unwrap();
    let pieces: i32 = args.next().unwrap().parse().unwrap();
    for _ in 0..n {
        // capacity doubling from 16 code units, as main.c grows its buffer
        let mut acc: Vec<u16> = Vec::with_capacity(16);
        for _ in 0..pieces {
            // acc = acc + piece on a uniquely owned acc appends in place
            acc.extend_from_slice(&PIECE);
        }
        total = total + acc.len() as i64;
    }
    println!("{}", total);
}
