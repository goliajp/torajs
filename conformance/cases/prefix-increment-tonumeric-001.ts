// §13.4.6 / §13.4.7 — `++x` / `--x` are UpdateExpressions: ToNumeric
// on the old value, one step in its own numeric domain, the NEW value
// answered. The parser desugared the prefix forms to `x = x + 1`,
// which is `+`, not ToNumeric: on an `any` holding `"1"` it
// concatenated to `"11"`; a boolean / null went the same way. The
// prefix form is the same UpdateExpression node as the postfix one
// now, answering the stepped value on every lane — typed locals,
// `any` locals, an any receiver's member, a keyed element, an array
// length, a BigInt.
let s: any = "1";
console.log(++s, s, --s, s);
let b: any = true;
console.log(++b, b);
let n: any = null;
console.log(++n, n);
let u: any = undefined;
console.log(++u);
let big: any = 10n;
console.log(++big, big, --big);
let d: any = "1.5";
console.log(--d, d);
let i = 7;
console.log(++i, i--, i, --i, i);
let f = 1.5;
console.log(++f, f);
const o: any = { x: "4" };
console.log(++o.x, o.x, --o.x);
const a: any = ["9"];
console.log(++a[0], a[0], --a[0]);
const xs = [1, 2, 3];
console.log(++xs[1], xs[1], --xs.length, xs);
let k = 0;
let acc = 0;
for (let j = 0; j < 5; ++j) { acc += j; ++k; }
console.log(acc, k);
const get: any = { v: "2" };
console.log([++get.v, get.v++, get.v]);
