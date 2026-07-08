// variadic fn-type long-tail lanes (RFC 20260708-variadic chunk 3):
// heap-typed returns transfer out of the boxed dual entry, a bare
// named fn wraps into the closure world via the forwarder pass, a
// rest-tail struct field dispatches boxed instead of static, and an
// unannotated binding inferred from a rest-tail return registers
// for the boxed lane.
const t: (...xs: string[]) => string = (a: string) => a + "!";
console.log(t("hi"));
console.log(t("a", "b"));
function named(a: number, b: number): number { return a * b; }
function h(cb: (...args: any[]) => number): number { return cb(6, 7); }
console.log(h(named));
interface Box { fn: (...args: number[]) => number; }
const b: Box = { fn: (x: number) => x + 1 };
console.log(b.fn(41));
console.log(b.fn(41, 99));
function mk(): (...args: number[]) => number {
  return (a: number, b2: number) => (a ?? 0) + (b2 ?? 0);
}
const f = mk();
console.log(f(40, 2));
console.log(f(42));
