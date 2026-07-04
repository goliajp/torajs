// any-method-call RFC C4+ — bare call on an any-typed callee:
// closures reach their boxed dual entry through __torajs_any_call;
// non-functions raise a catchable TypeError.
const f: any = (x: number) => x + 1;
console.log(f(1));
const g: any = (a: number, b: number) => a * b;
console.log(g(3, 4));
// captured environment rides the env pointer
let base = 10;
const h: any = (x: number) => x + base;
console.log(h(5));
// zero-arg + extra args ignored
const z: any = () => "zed";
console.log(z());
console.log(f(1, 99));
// any args pass through unboxed
const id: any = (v: any) => v;
console.log(id("str"));
console.log(id(2.5));
// string arg into a typed param
const shout: any = (s: string) => s.toUpperCase();
console.log(shout("quiet"));
// call result reassigned and re-called
const dbl: any = (x: number) => x * 2;
const alias: any = dbl;
console.log(alias(21));
// non-function callees throw catchably
try {
  const n: any = 5;
  n(1);
} catch (e) {
  console.log("caught");
}
try {
  const s: any = "hi";
  s();
} catch (e) {
  console.log("caught2");
}
console.log("done");
