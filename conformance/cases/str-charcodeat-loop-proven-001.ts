// `s.charCodeAt(i)` under a `i < s.length` guard over a counter that
// starts at a non-negative literal and only steps up is always in
// range, so its answer is an integer code unit: compared with integer
// literals, stored into class fields, used as an array index.
class Tok {
  kind: number;
  value: number;
  constructor(kind: number, value: number) {
    this.kind = kind;
    this.value = value;
  }
}

function tokenize(src: string): Tok[] {
  const out: Tok[] = [];
  let i = 0;
  while (i < src.length) {
    const c = src.charCodeAt(i);
    if (c === 32) {
      i = i + 1;
    } else if (c >= 48 && c <= 57) {
      let v = 0;
      while (i < src.length) {
        const d = src.charCodeAt(i);
        if (d < 48 || d > 57) break;
        v = v * 10 + (d - 48);
        i = i + 1;
      }
      out.push(new Tok(0, v));
    } else {
      out.push(new Tok(1, c));
      i = i + 1;
    }
  }
  return out;
}

function sumUnits(s: string): number {
  let total = 0;
  for (let i = 0; i < s.length; i++) {
    total = total + s.charCodeAt(i);
  }
  return total;
}

const table: number[] = [10, 20, 30, 40, 50, 60, 70, 80];
function lookup(s: string): number {
  let acc = 0;
  for (let i = 0; i < s.length; i = i + 1) {
    acc = acc + table[s.charCodeAt(i) - 97];
  }
  return acc;
}

for (const t of tokenize("12 + x*(7 - 3)")) console.log(t.kind, t.value);
console.log(sumUnits("hello"), sumUnits(""), sumUnits("中文😀"));
console.log(lookup("abcdefgh"), lookup("hah"), lookup(""));

// a same-named counter in another loop stepping the other way keeps
// this one honest: the guard here still proves nothing about the
// lower bound, so the read must answer NaN for the negative index
let i = -2;
while (i < "ab".length) {
  console.log("neg", i, "ab".charCodeAt(i));
  i = i + 1;
}
for (let j = 3; j > 0; j = j - 1) console.log("down", j);

// a counter written negative behind the loop's back, once
let k = 0;
let armed = true;
function reset(): void {
  k = -2;
}
const w = "xyz";
while (k < w.length) {
  console.log("k", k, w.charCodeAt(k));
  if (k === 1 && armed) {
    armed = false;
    reset();
  }
  k = k + 1;
}

// the guard on a different string than the receiver proves nothing
const a = "abcdef";
const b = "ab";
for (let m = 0; m < a.length; m = m + 1) console.log("mixed", m, b.charCodeAt(m));

// a wide string read through the same proven loop keeps its halfwords
const wide = "aé中😀";
let units = "";
for (let n = 0; n < wide.length; n = n + 1) {
  const u = wide.charCodeAt(n);
  units = units + (u === 0x4e2d ? "*" : u.toString(16)) + ",";
}
console.log(units);
