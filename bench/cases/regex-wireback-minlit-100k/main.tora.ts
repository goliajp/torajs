let total: number = 0;
let n: number = 100000;
const re: RegExp = /x/;
for (let i: number = 0; i < n; i = i + 1) {
  let m: string[] | null = "x".match(re);
  if (m !== null) total = total + 1;
}
console.log(total);
