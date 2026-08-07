// An assignment expression's value is its rhs (§13.15.2 step 8).
//
// Five lanes of the member-assign ladder answered the integer 0
// instead: `b = (o.k = [1,2,3])` left `b` holding 0, silently. The
// Ident-target lane got its value (and its ownership contract) in
// rotation 323; these join it — the consumer receives an owned
// reference, minted before the rhs temp's own release.

// dynobj lane, each rhs shape
var o: any = {};
var b1: any = 0;
b1 = (o.k = [1, 2, 3]);
console.log(String(b1));

var b2: any = 0;
b2 = (o.m = "txt");
console.log(String(b2));

var b3: any = 0;
b3 = (o.n = 7);
console.log(String(b3));

var b4: any = 0;
b4 = (o.f = 2.5);
console.log(String(b4));

var b5: any = 0;
b5 = (o.t = true);
console.log(String(b5));

// an Any rhs — this one always worked in the borrow spelling; the
// owned-temp spelling must survive the temp's release
var av: any = [9, 9];
var b6: any = 0;
b6 = (o.a = av);
console.log(String(b6));

function wrap(x: any): any {
  return x;
}
var b7: any = 0;
b7 = (o.w = wrap([8, 7]));
console.log(String(b7));

// a call rhs
function five(): number {
  return 5;
}
var b8: any = 0;
b8 = (o.c = five());
console.log(String(b8));

// null / undefined rhs pass through the mint untouched
var b9: any = 1;
b9 = (o.z = null);
console.log(String(b9));
var b10: any = 1;
b10 = (o.u = undefined);
console.log(String(b10));

// chained through a second member write
var b11: any = 0;
b11 = (o.k2 = o.k3 = "deep");
console.log(String(b11));

// closure-props lane
function fn0(): number {
  return 0;
}
var b12: any = 0;
b12 = ((fn0 as any).tag = "fp");
console.log(String(b12));

// array length lane answers the assigned number
var arr = [1, 2, 3, 4];
var b13: any = 0;
b13 = (arr.length = 2);
console.log(String(b13), arr.length);

// the value is usable, not just printable
var sum = 0;
var acc: any = {};
sum = (acc.v = 20) + 5;
console.log(sum);

// statement-position writes exercise the discard face; the loop must
// complete and later writes still land. (Reading a heap value BACK
// from an any-receiver bucket after a statement-position array write
// is a pre-existing defect independent of the expression's value —
// `o.x = [7,8]; String(o.x)` answers empty on the baseline tree too —
// so this pins liveness, not that read.)
for (let i = 0; i < 3; i++) {
  o.loop = [i, i];
}
o.after = "alive";
console.log(String(o.after));
