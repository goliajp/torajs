// An arrow created inside a named function reads a top-level `const`
// initialized with a number or boolean literal, directly or through a
// higher-order call; a local of the same name in between wins.

const k = 5;
const half = 0.5;
const on = true;
const neg = -2;

function ap(f: (x: number) => number): number {
  return f(3);
}
function local(): number {
  const h = (x: number) => x * k;
  return h(3);
}
function viaArg(): number {
  return ap((x) => x * k + half);
}
function nested(f: (h: (x: number) => number) => number): number {
  return f((x) => (on ? x * k : 0) + neg);
}
function shadowed(): number {
  const k = 7;
  return ap((x) => x * k);
}

console.log(local(), viaArg(), nested((h) => h(3)), shadowed());
console.log(ap((x) => x + k), k, half, on);
