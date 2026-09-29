fn next(s: i64) -> i64 {
    let t = ((s * 48271) as i32 & 0x7fffffff) as i64;
    if t == 0 {
        1
    } else {
        t
    }
}

struct Match {
    index: usize,
    key_end: usize,
    end: usize,
}

// ([a-z]+)=(\d+): every start inside one run of letters reaches the same end,
// so a run that fails is skipped whole
fn find(s: &[u8], from: usize) -> Option<Match> {
    let mut p = from;
    while p < s.len() {
        if !s[p].is_ascii_lowercase() {
            p += 1;
            continue;
        }
        let mut q = p + 1;
        while q < s.len() && s[q].is_ascii_lowercase() {
            q += 1;
        }
        if q + 1 < s.len() && s[q] == b'=' && s[q + 1].is_ascii_digit() {
            let mut r = q + 2;
            while r < s.len() && s[r].is_ascii_digit() {
                r += 1;
            }
            return Some(Match {
                index: p,
                key_end: q,
                end: r,
            });
        }
        p = q;
    }
    None
}

fn replacer(k: &str, v: &str) -> String {
    let mut r = String::new();
    r.push_str(v);
    r.push(':');
    r.push_str(k);
    r
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().unwrap().parse().unwrap();
    let fields: usize = args.next().unwrap().parse().unwrap();
    let mut seed: i64 = args.next().unwrap().parse().unwrap();

    let mut lines: Vec<String> = Vec::new();
    for _ in 0..1024 {
        let mut line = String::new();
        for _ in 0..fields {
            seed = next(seed);
            let shape = seed % 8;
            seed = next(seed);
            let klen = 1 + seed % 8;
            let mut key = String::new();
            for _ in 0..klen {
                seed = next(seed);
                key.push((b'a' + (seed % 26) as u8) as char);
            }
            seed = next(seed);
            let vlen = 1 + seed % 6;
            let mut value = String::new();
            for _ in 0..vlen {
                seed = next(seed);
                value.push((b'0' + (seed % 10) as u8) as char);
            }
            if shape != 1 {
                line.push_str(&key);
            }
            if shape != 0 {
                line.push('=');
            }
            line.push_str(&value);
            line.push(';');
        }
        lines.push(line);
    }

    let mut total: i64 = 0;
    for i in 0..n {
        let line = &lines[i & 1023];
        let mut out = String::new();
        let mut last = 0;
        while let Some(m) = find(line.as_bytes(), last) {
            let key = String::from(&line[m.index..m.key_end]);
            let value = String::from(&line[m.key_end + 1..m.end]);
            let rep = replacer(&key, &value);
            out.push_str(&line[last..m.index]);
            out.push_str(&rep);
            last = m.end;
        }
        out.push_str(&line[last..]);
        total += out.len() as i64 + out.as_bytes()[out.len() >> 1] as i64;
    }
    println!("{}", total);
}
