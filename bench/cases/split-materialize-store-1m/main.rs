fn next(s: i64) -> i64 {
    let t = (s * 48271) % 2147483647;
    if t == 0 {
        1
    } else {
        t
    }
}

// the split of main.c: count the separators, allocate the slice array
// once at its final size, then fill it with views into the line
fn split(line: &str) -> Vec<&str> {
    let b = line.as_bytes();
    let mut parts = Vec::with_capacity(b.iter().filter(|&&c| c == b',').count() + 1);
    let mut start = 0;
    for (i, &c) in b.iter().enumerate() {
        if c == b',' {
            parts.push(&line[start..i]);
            start = i + 1;
        }
    }
    parts.push(&line[start..]);
    parts
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().unwrap().parse().unwrap();
    let fields: usize = args.next().unwrap().parse().unwrap();
    let mut seed: i64 = args.next().unwrap().parse().unwrap();

    let mut lines: Vec<String> = Vec::new();
    for _ in 0..1024 {
        let mut line = String::new();
        for f in 0..fields {
            seed = next(seed);
            if f > 0 {
                line.push(',');
            }
            line.push_str(&(seed % 100000).to_string());
        }
        lines.push(line);
    }

    let mut rows: Vec<Vec<&str>> = vec![Vec::new(); 1024];

    let mut total: i64 = 0;
    for i in 0..n {
        let parts = split(&lines[i & 1023]);
        // capacity doubling from 16, as main.c grows rebuilt
        let mut rebuilt = String::with_capacity(16);
        for p in &parts {
            total += p.as_bytes()[p.len() - 1] as i64;
            rebuilt.push_str(p);
            rebuilt.push(';');
        }
        total += rebuilt.len() as i64;
        rows[i & 1023] = parts;
    }

    let mut held: i64 = 0;
    for row in &rows {
        held += row.len() as i64 + row[row.len() - 1].len() as i64;
    }
    println!("{}", total);
    println!("{}", held);
}
