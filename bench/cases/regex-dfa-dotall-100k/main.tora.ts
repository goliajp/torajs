let total: number = 0;
let n: number = 100000;
const re: RegExp = /a.+c/s;
for (let i: number = 0; i < n; i = i + 1) {
  let s: string = "pre a\nmiddle\nc post " + i.toString();
  let m: string[] | null = s.match(re);
  if (m !== null) total = total + m[0].length;
}
console.log(total);
