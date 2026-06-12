// chunk 2.5 F2 (ann-width rfc §5.6) — synthetic fn (`__cm_*` /
// `__closure_*`) ret/param widths consult num_width like user fns.
// Pre-F2 the lowering consumer sites skipped `__`-prefixed fns,
// pinning their `: number` faces to I64: a class method returning
// x/2 truncated through the FpToSi return arm (1 instead of 1.5),
// an arrow did the same through its own closure sig (0 instead of
// 0.75). Int-bodied methods/arrows must hold the I64 ABI.
class A {
  half(x: number): number { return x / 2; }
  twice(x: number): number { return x * 2; }
  scale(f: number): number { f = f / 2; return f * 10; }
}
const a = new A();
console.log(a.half(3));
console.log(a.twice(3));
console.log(a.scale(5));

const g = (x: number): number => x / 4;
console.log(g(3));
const k = (x: number): number => x + 1;
console.log(k(3));
