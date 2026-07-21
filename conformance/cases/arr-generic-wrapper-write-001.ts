// RFC 20260721-array-proto-cluster 刀 9 — G2c primitive-wrapper
// indexed write (decimal key → member-set wrapper arm's lazy expando
// dynobj) + the generic array-like read family over the wrapper's
// OWN expando face (length + digit keys).

const b: any = new Boolean(false);
b.length = 2;
b[1] = true;
console.log("bool:", Array.prototype.indexOf.call(b, true), Array.prototype.lastIndexOf.call(b, true), b[1]);

const n: any = new Number(-3);
n.length = 2;
n[1] = true;
console.log("num:", Array.prototype.indexOf.call(n, true), n[1], n.length);

// in-range StringWrapper index domain stays non-writable.
const s: any = new String("ab");
try {
  s[0] = "x";
  console.log("strw: wrote", s[0]);
} catch (e: any) {
  console.log("strw: threw", s[0]);
}

// generic includes/at over the wrapper own face.
console.log("inc:", Array.prototype.includes.call(b, true), Array.prototype.at.call(b, 1));
