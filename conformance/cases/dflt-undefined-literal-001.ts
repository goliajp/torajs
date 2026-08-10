// ES §10.2.11 step 26 — an explicit `undefined` argument binds the
// parameter default exactly like a missing one, across every call
// shape the call-site padding table serves (plain fn / trailing pos /
// leading pos with a later actual / arrow alias / class method /
// obj-literal method).
function f(x: number = 5): number {
  return x;
}
console.log(f(undefined));
function g(a: number, b: boolean = true): boolean {
  return b;
}
console.log(g(1, undefined));
function m(a: number = 1, b: number = 0): number {
  return a + b;
}
console.log(m(undefined, 7));
const c = (x: number = 2.5): number => x;
console.log(c(undefined));
class A {
  m(x: number = 3): number {
    return x;
  }
}
console.log(new A().m(undefined));
const o = {
  m(x: string = "d"): string {
    return x;
  },
};
console.log(o.m(undefined));
