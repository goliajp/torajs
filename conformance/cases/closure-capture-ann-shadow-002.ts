// plain-fn declaration-order variant of 001: a LATER fn's `x: any` /
// `x: string` param used to win the by-name last-seen race and flip
// the closure's sniffed ret ann (`x: string` was a checker error:
// "function expects String, got Number").
function viaParam(x: number): number {
  const f: () => number = () => x;
  return f();
}
function isErrAny(x: any): boolean {
  return x === 42;
}
function isErrStr(x: string): boolean {
  return x.length > 0;
}
console.log(viaParam(7));
console.log(isErrAny(42));
console.log(isErrStr("hi"));
