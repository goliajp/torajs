// an any[] read at a number index that is not an i64 at compile time
// (`i % 3`, a float): an integer reads the element, a non-integer
// names the property "1.5" (an expando, or undefined).
const a: any[] = ["x", 7, { k: 2 }];
let out = "";
for (let i = 0; i < 6; i = i + 1) {
  const v = a[i % 3];
  out = out + (typeof v === "object" ? "o" + v.k : String(v)) + ",";
}
console.log(out);
const h = 0.5;
console.log(a[h], a[1 + h], a[2 / 2]);
(a as any)[1.5] = "p";
console.log(a[1 + h], a.length);
const words: any[] = [];
for (let j = 0; j < 50; j = j + 1) words.push("w" + j);
let total = 0;
for (let i = 0; i < 20000; i = i + 1) {
  const s = words[i % 50];
  total = total + s.length;
}
console.log(total);
