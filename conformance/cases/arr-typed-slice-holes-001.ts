// slice over a typed receiver keeps its holes as holes of the product
// (§23.1.3.28 step 12.b); a dense receiver copies as before.
let a = [1.5, 2.5];
a[4] = 3.5;
console.log(a.slice(1), a.slice(-2), a.slice(3, 4), a.slice(0, 2));
const s = a.slice(1);
console.log(1 in s, 2 in s, s.length, s[1], s.filter(() => true));
let g = [7, 8];
g.length = 4;
console.log(g.slice(1), g.slice());
const t = ["a"];
t[2] = "c";
console.log(t.slice(), t.slice(1));
const d = [1.5, 2.5, 3.5];
console.log(d.slice(1), d.slice(-1));
