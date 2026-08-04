// RFC 20260804-fn-this-channel knife 1 — .call/.apply thisArg reaches
// a this-using FnDecl's __this param; this-free targets keep the drop.
function f() {
  return (this as any)._v;
}
console.log(f.call({ _v: 9 }));

function g(a: number, b: number) {
  return a + b;
}
console.log(g.call(null, 3, 4));

function h() {
  return (this as any).x + 1;
}
console.log(h.apply({ x: 41 }, []));

function k(s: string) {
  return (this as any).tag + s;
}
console.log(k.apply({ tag: "T-" }, ["ok"]));
