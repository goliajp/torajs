// `f.call(t, ...)` / `f.apply(t, [...])` on a function value answers
// what the function returns, and hands it the arguments after the
// receiver: a fraction stays a fraction on both sides, through a
// parameter, a generic parameter, a local and a function expression.
function viaParam(g: (x: number) => number): number {
  return g.call(null, 0.75);
}
function viaParamApply(g: (x: number, y: number) => number): number {
  return g.apply(null, [0.25, 1.5]);
}
function viaGeneric<F extends (x: number) => number>(f: F): number {
  return f.call(null, 0.75);
}
function viaGenericApply<F extends (x: number, y: number) => number>(f: F): number {
  return f.apply(null, [0.25, 1.5]);
}
function viaLocal<F extends (x: number) => number>(f: F): number {
  const h = f;
  return h.call(null, 0.5);
}
const viaArrow = <F extends (x: number, y: number) => number>(f: F, y: number): number =>
  f.apply(null, [0.5, y]);
function k1(x: number): number {
  return x * 3;
}
function k2(x: number, y: number): number {
  return x + y;
}
function k3(x: number): number {
  return x * 5;
}
function k4(x: number, y: number): number {
  return x * y;
}
function k5(x: number): number {
  return x * 9;
}
function k6(x: number, y: number): number {
  return x - y;
}
console.log(viaParam(k1), viaParamApply(k2), viaGeneric(k3), viaGeneric(k1));
console.log(viaGenericApply(k4), viaLocal(k5), viaArrow(k6, 0.125));
const sq = (x: number): number => x * x;
console.log(viaParam(sq), sq.call(null, 1.5), sq.apply(null, [2.5]));
