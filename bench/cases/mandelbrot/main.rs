fn mandel(cr: f64, ci: f64, max_iter: u32) -> u32 {
    let mut zr = 0.0_f64;
    let mut zi = 0.0_f64;
    let mut n = 0_u32;
    while n < max_iter {
        if zr * zr + zi * zi > 4.0 {
            return n;
        }
        let new_zr = zr * zr - zi * zi + cr;
        zi = 2.0 * zr * zi + ci;
        zr = new_zr;
        n += 1;
    }
    max_iter
}

fn main() {
    let mut total: u64 = 0;
    for i in 0..200 {
        for j in 0..200 {
            let cr = i as f64 / 100.0 - 1.5;
            let ci = j as f64 / 100.0 - 1.0;
            total += mandel(cr, ci, 1000) as u64;
        }
    }
    println!("{}", total);
}
