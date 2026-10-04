// Fill loops whose pushes sit under an if: each trip appends what the
// branch it takes appends, and the array ends up exactly that long.
function num(s: string): number {
  return parseInt(s, 10);
}
const n = num("10");

let a: number[] = [];
let i = 0;
while (i < n) {
  if (i % 2 === 0) {
    a.push(i);
  } else {
    a.push(-i);
  }
  i = i + 1;
}
console.log(a.length, a.join(","));

// unequal branches, a missing else, two arrays
let b: number[] = [];
let c: number[] = [];
for (let k = 0; k < n; k++) {
  if (k % 3 === 0) {
    b.push(k);
    b.push(k * 10);
  } else if (k % 3 === 1) {
    c.push(k);
  }
  if (k > 7) b.push(-k);
}
console.log(b.length, b.join(","), c.length, c.join(","));

// a condition that reads the array being filled: never deferred
let d: number[] = [];
let j = 0;
while (j < n) {
  if (d.length % 2 === 0) {
    d.push(j);
  } else {
    d.push(d.length);
  }
  j = j + 1;
}
console.log(d.join(","));

// a condition that calls code which pushes too
let e: number[] = [];
function sneak(v: number): boolean {
  if (v === 3) e.push(100);
  return v % 2 === 0;
}
let m = 0;
while (m < 6) {
  if (sneak(m)) {
    e.push(m);
  } else {
    e.push(-m);
  }
  m = m + 1;
}
console.log(e.length, e.join(","));

// a frozen receiver refuses the first append the loop makes
let f: number[] = [];
Object.freeze(f);
try {
  let q = 0;
  while (q < 3) {
    if (q > 0) {
      f.push(q);
    } else {
      f.push(-q - 1);
    }
    q = q + 1;
  }
} catch (err) {
  console.log((err as Error).constructor.name, f.length);
}

// a zero-trip loop leaves the array alone
let g: number[] = [7];
let r = 0;
const none = num("0");
while (r < none) {
  if (r) g.push(r);
  r = r + 1;
}
console.log(g.length);
