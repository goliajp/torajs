// Unannotated fn-expr / arrow callbacks on typed Map/Set receivers:
// infer_anonymous_closure_params seeds the param types from the
// receiver's instantiation — Map<K, V> forEach cb is (V, K, Map),
// Set<T> is (T, T, Set). The receiver ann comes from an explicit let
// ann or from `new Map<K, V>()` type args now carried on Expr::New.
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
const out: string[] = [];
m.forEach(function (v, k) {
  out.push(k + "=" + v * 10);
});
console.log(out.join(","));
const s = new Set<number>();
s.add(3);
s.add(4);
let sum = 0;
s.forEach(function (v) {
  sum += v * 2;
});
console.log(sum);
const m2: Map<string, number> = new Map();
m2.set("x", 7);
m2.forEach(function (v) {
  console.log(v + 1);
});
const m3 = new Map<string, number[]>();
m3.set("xs", [1, 2, 3]);
m3.forEach(function (v, k) {
  console.log(k, v.length);
});
const arrows = new Map<string, number>();
arrows.set("z", 5);
arrows.forEach((v) => console.log(v * 3));
