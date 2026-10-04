// `-x` under a guard that rules out x == 0 is an exact integer; every
// negation that can still see a zero keeps answering -0.
function show(label: string, v: number): void {
  console.log(label, v, 1 / v, Object.is(v, -0));
}

let xs: number[] = [];
let i = 0;
while (i < 6) {
  if (i % 2 === 0) {
    xs.push(i);
  } else {
    xs.push(-i);
  }
  i = i + 1;
}
console.log(xs.join(","));

// the even branch negates, and 0 is even: -0 lands in the array
let ys: number[] = [];
let k = 0;
while (k < 3) {
  if (k % 2 === 0) {
    ys.push(-k);
  } else {
    ys.push(k);
  }
  k = k + 1;
}
show("ys0", ys[0]);
console.log(String(ys[0]), ys.join(","));

// the guard forms that do rule zero out
let a = 0;
let out: number[] = [];
while (a < 4) {
  if (a > 0) out.push(-a);
  if (a !== 0) out.push(-a);
  if (a) out.push(-a);
  if (!(a === 0)) out.push(-a);
  if (a >= 1 && a < 3) out.push(-a);
  if (a <= 0) {
    out.push(a);
  } else {
    out.push(-a);
  }
  if (0 < a) out.push(-a);
  if (a % 3 !== 0) out.push(-a);
  a = a + 1;
}
console.log(out.join(","));

// and the ones that do not
let b = 0;
let neg: number[] = [];
while (b < 2) {
  if (b >= 0) neg.push(-b);
  if (b < 1) neg.push(-b);
  if (b === 0) neg.push(-b);
  if (b % 0 !== 0) neg.push(-b);
  if (b !== 0 || b === 0) neg.push(-b);
  b = b + 1;
}
console.log(neg.map((v) => (Object.is(v, -0) ? "-0" : String(v))).join(","));

// a write between the guard and the negation
let c = 3;
let w: number[] = [];
if (c !== 0) {
  c = 0;
  w.push(-c);
}
let d = 5;
if (d !== 0 && (d = 0) === 0) {
  w.push(-d);
}
let g = 7;
function resetG(): number {
  g = 0;
  return 1;
}
if (g !== 0) {
  resetG();
  w.push(-g);
}
let h = 9;
const clearH = (): void => {
  h = 0;
};
if (h !== 0) {
  clearH();
  w.push(-h);
}
for (const v of w) show("w", v);

// a guard on a value that is not a number
let s: any = "0";
if (s !== 0) show("s", -s);
