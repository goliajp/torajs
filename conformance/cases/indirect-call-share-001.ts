// chunk 569 — indirect/vtable call args always SHARE (RFC 20260705 ledger #3):
// no caller-side inc (the callee body never drops params) and no consume
// (source keeps its stake); owned temps release after the call.

// 1. closure-typed local
let f = (s: string): string => s + "!";
let a = "AAAA" + 1;
let r1 = f(a);
let r2 = f(a);
console.log(a);
console.log(r1);
console.log(r2);
console.log(f("BBBB" + 2));

// 2. fnsig-typed local
function g(s: string): string { return s + "?"; }
let h = g;
let b = "CCCC" + 3;
console.log(h(b));
console.log(b);
console.log(h("DDDD" + 4));

// 3. IIFE closure literal callee
let c = "EEEE" + 5;
console.log(((x: string): string => x + "~")(c));
console.log(c);

// 4. chained call callee
function mk(): (x: string) => string {
  const inner = (x: string): string => x + "*";
  return inner;
}
let d = "FFFF" + 6;
console.log(mk()(d));
console.log(d);

// 5. vtable dispatch args share
class VA {
  m(s: string): string { return s; }
}
class VB extends VA {
  m(s: string): string { return s + "#"; }
}
let o: VA = new VB();
let e = "GGGG" + 7;
console.log(o.m(e));
console.log(e);
console.log(o.m("HHHH" + 8));
