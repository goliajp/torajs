// §13.3.8 / §13.3.2 — call Arguments and bracketed member
// expressions re-enter at [+In]: the for-head restriction stops at
// those boundaries too (401-04, the literal-boundary fix's sibling).
function f(b: any): any {
  return b;
}
var y: any = { a: 1 };
var x: any, w: any;
for (x = f("a" in y); false; ) ;
console.log(x);
for (w = y[("a" in y) as any]; false; ) ;
console.log(w);
