// rotation 362 — `any ?? T` joins to Any (§13.4.2: a non-nullish lhs
// IS the result, whatever it holds). The old rhs-width unbox read an
// f64 box's raw bits as an i64 and collapsed non-bool payloads
// through a != 0 test.
function g(a: number, args2: any[]) {
  return args2[0] ?? a;
}
console.log(g(1, [2])); // was 4611686018427387904 (f64 2.0 bits)
console.log(g(7, [])); // nullish path takes the rhs

const xs: any[] = [0, false, "", null];
console.log(xs[0] ?? true); // 0 — falsy but not nullish
console.log(xs[1] ?? true); // false
console.log(xs[2] ?? "dflt"); // "" — empty string is not nullish
console.log(xs[3] ?? "dflt"); // null IS nullish

function h(flag: boolean, src: any[]) {
  return src[0] ?? flag;
}
console.log(h(true, [0])); // 0 — was false through the != 0 squeeze

function rest1(a: number, ...args: any[]) {
  return args[0] ?? a;
}
console.log(rest1(44)); // 44 — empty rest, nullish path
console.log(rest1(1, 2)); // 2 — rest element rides the Any join
