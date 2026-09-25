// map over a typed source with holes keeps them in the product
// (§23.1.3.21 step 6.c leaves the index uncreated): a number, boolean,
// string or object product marks the hole over its element type's hole
// word instead of calling the callback on it.
function mk(): number[] {
  let a = [1.5, 2.5];
  a[4] = 3.5;
  return a;
}
console.log(mk().map((v) => v * 2));
console.log(mk().map((v) => v > 2));
console.log(mk().map((v) => "s" + v));
console.log(mk().map((v) => ({ v })).length);
const r = mk().map((v) => v * 2);
console.log(2 in r, r[2], r.length, JSON.stringify(r));
const b = [true];
b[2] = false;
console.log(b.map((x) => !x));
const s = ["a"];
s[2] = "c";
const u = s.map((x) => x + "!");
console.log(u, 1 in u, u[1]);
console.log([1, 2, 3].map((x) => x * 2), [1.5].map((x) => x > 1));
