let total: number = 0;
let n: number = 100000;
const re: RegExp = /a(b+)c/g;
for (let i: number = 0; i < n; i = i + 1) {
  let s: string = "abbc xxx abc yyy abbbbc";
  let r: string = s.replace(re, "XY");
  total = total + r.length;
}
console.log(total);
