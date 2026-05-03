fn eval_rpn(expr: &str) -> i64 {
    let mut stack: [i64; 16] = [0; 16];
    let mut sp: usize = 0;
    for tok in expr.split(' ') {
        let c0 = tok.as_bytes()[0] as i64;
        if c0 >= 48 && c0 <= 57 {
            stack[sp] = c0 - 48;
            sp += 1;
        } else {
            let b = stack[sp - 1];
            let a = stack[sp - 2];
            sp -= 2;
            let r = if c0 == 43 {
                a + b
            } else if c0 == 45 {
                a - b
            } else {
                a * b
            };
            stack[sp] = r;
            sp += 1;
        }
    }
    stack[0]
}

fn main() {
    let mut total: i64 = 0;
    let n: i64 = 100_000;
    for _ in 0..n {
        total += eval_rpn("3 4 + 2 * 5 +");
    }
    println!("{}", total);
}
