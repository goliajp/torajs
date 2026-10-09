// A local, a parameter, a block binding or a captured binding named
// like a global function property shadows it for a bare call
// (§9.1.1.1); outside that scope the builtin answers again.

function locals(): void {
  let parseFloat = (s: string): number => s.length + 40;
  const decodeURIComponent = (s: string): string => "dec:" + s;
  console.log(parseFloat("1.5"), decodeURIComponent("%41"));
  console.log(globalThis.parseFloat("1.5"), globalThis.decodeURIComponent("%41"));
}
locals();

function params(isFinite: (x: number) => string, escape: (s: string) => number): void {
  console.log(isFinite(Infinity), escape("a b"));
}
params((x: number) => "fin" + x, (s: string) => s.length);

function captured(): void {
  const isNaN = (x: number): string => "cap" + x;
  const inner = (): string => isNaN(7);
  console.log(inner());
  [1, 2].forEach((v: number) => console.log(isNaN(v)));
}
captured();

{
  let encodeURIComponent = (s: string): string => s + "!";
  console.log(encodeURIComponent("x y"));
}

var queueMicrotask = (f: () => void): number => 9;
console.log(queueMicrotask(() => {}));

console.log(escape("a b"), isFinite(1), parseFloat("2.5"), encodeURIComponent("x y"), isNaN(1));
