// RFC 20260714-dstr-residual — array-literal elision holes are
// `undefined` per ES §13.2.4 (they were `null` placeholders from the
// pre-undefined era). Observable through direct reads, destructuring
// defaults (fire on undefined, NOT on explicit null), and for-of.

let a = [,];
console.log(a.length);

let b = [1, , 3];
console.log(b.length, typeof b[1], b[0], b[2]);

let s: any = [, "x"];
console.log(s.length, s[0], s[1]);

// hole fires the destructuring default; explicit null must not
function f([x = 23]: any) {
  console.log("x=", x);
}
f([,]);
f([null]);
f([undefined]);

for (const v of [1, , 3]) {
  console.log("v:", v);
}
console.log("done");
