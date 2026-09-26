// js strings are UTF-16 and .length counts code units, so the string is a
// Vec<u16> here; a UTF-8 String would copy 3 bytes per char instead of 2 and
// need a scan to get the code-unit length
fn concat(a: &[u16], b: &[u16]) -> Vec<u16> {
    // a + b builds a fresh string of exactly the combined length
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    out
}

fn main() {
    let mut total: i64 = 0;
    let piece: Vec<u16> = "中文测试".encode_utf16().collect();
    let n: i64 = 1000;
    for _ in 0..n {
        let mut acc: Vec<u16> = Vec::new();
        for _ in 0..100 {
            acc = concat(&acc, &piece);
        }
        total = total + acc.len() as i64;
    }
    println!("{}", total);
}
