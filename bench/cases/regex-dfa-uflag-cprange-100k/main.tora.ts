let total: number = 0;
let n: number = 100000;
const re: RegExp = /[A-Za-z]+/u;
for (let i: number = 0; i < n; i = i + 1) {
  let s: string = "  Hello42 word  " + i.toString();
  let m: string[] | null = s.match(re);
  if (m !== null) total = total + m[0].length;
}
console.log(total);
