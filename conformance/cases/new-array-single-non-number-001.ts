// §23.1.1.1 step 4 — `new Array(len)`: a Number argument is the length;
// any other value becomes the single element (length 1).
const a = new Array("3");
console.log(a.length, a[0], typeof a[0]);
console.log(new Array(true), new Array(null), new Array(undefined).length);
const o = { k: 1 };
const b = new Array(o);
console.log(b.length, b[0] === o);
const u: number | undefined = undefined;
const c = new Array(u);
console.log(c.length, c[0]);
const w: number | undefined = 2;
console.log(new Array(w).length);
console.log(new Array(3).length, new Array(2.0).length);
try {
  new Array(2.5);
} catch (e) {
  console.log(e instanceof RangeError);
}
console.log(Array("x").length);
