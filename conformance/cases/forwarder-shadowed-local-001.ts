// rotation 128 — the fn-to-closure forwarder collector must not treat
// a shadowing LOCAL as a value use of a same-named top-level fn: the
// eq-operand axis (rotation 127) rewrote `const f: string` reads
// inside an unrelated fn into `__forward_f`, and the forwarder of a
// destructured-param fn rejected the whole program (test262
// propertyHelper shape, 27 dstr cases).
function f([x, y, z]) {
  console.log(x, y, z);
}
function unrelated(names: string[]): void {
  for (let i = 0; i < names.length; i++) {
    const f: string = names[i];
    if (f !== "value") {
      console.log("field", f);
    }
  }
}
f([1, 2, 3]);
unrelated(["a", "value", "b"]);
// A real value use of a typed-param fn still answers the canonical
// cell (the untyped-param forwarder synthesis gap is a separate
// pre-existing face — plan-state L3b).
function g(n: number): number {
  return n;
}
const t: any = g;
const u: any = g;
console.log(t === u, t === g);
console.log("done");
