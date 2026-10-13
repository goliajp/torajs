// A number-typed `+` operand inside an array literal stays on the
// numeric lane even when the field it reads shares its name with one
// that may hold the string undefined sentinel.

function* g() {
  let n = 1;
  const k = n;
  const w = [k + 7, 3];
  console.log(w[0], w[1]);
  const b = true;
  const x = [k + 0.5, n + k];
  console.log(x[0], x[1], b);
  yield 1;
  const s: string = "s" + k;
  const y = [s + k, s];
  console.log(y[0], y[1]);
}
for (const _ of g()) {}

function* h(m: number) {
  let i = 0;
  while (i < 2) {
    const v = m + i;
    yield i;
    const a = [v + 7, v * 2];
    console.log(a[0], a[1], a.length);
    i++;
  }
}
for (const _ of h(4)) {}
