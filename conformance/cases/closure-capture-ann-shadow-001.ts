// closure ret-ann sniff resolves captures at the construction site —
// a class method's same-named `x: any` param must not poison the
// captured `x: number` (num-width Captured-broadcast poison,
// tasks/2026-07-18: viaParam(7) printed denormal garbage).
class K {
  static isErr(x: any): boolean {
    return x === 42;
  }
}
function viaParam(x: number): number {
  const f: () => number = () => x;
  return f();
}
console.log(viaParam(7));
console.log(K.isErr(42));
console.log(K.isErr("no"));
