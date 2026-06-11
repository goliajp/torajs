// W1 follow-up (ann-width RFC §5.4 D3 regression-scan find) — lets
// declared inside top-level blocks/loops key as Global like every
// other main-fn binding: the analysis must resolve them or their
// widths silently read as NotNum and fractional values FpToSi-
// truncate on the way into callee params (the mandelbrot repro).

// f1 — for-body const with fract init flows into a call arg
function half(x: number): number {
  return x / 2;
}
let sum = 0;
for (let i = 0; i < 3; i = i + 1) {
  const cr = i / 100 - 1.5;
  sum = sum + half(cr);
}
console.log(sum);

// f2 — bare block let, fract write then read
{
  let t = 0.25;
  t = t + 0.5;
  console.log(t);
}

// f3 — while-body const through a call boundary
let w = 0;
let n = 2;
while (n > 0) {
  const step = n / 4;
  w = w + half(step);
  n = n - 1;
}
console.log(w);

// f4 — if-branch let
if (sum < 0) {
  const neg = sum * 0.5;
  console.log(neg);
}

// f5 — container ann on a block-local let (W4 channel through the
// same resolve path)
for (let k = 0; k < 1; k = k + 1) {
  const xs: number[] = [1, 2];
  xs[0] = 0.5;
  console.log(xs[0]);
}

// int hold — all-int block locals keep integer printing
for (let j = 0; j < 2; j = j + 1) {
  const dbl = j * 2;
  console.log(dbl);
}
