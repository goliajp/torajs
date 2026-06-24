let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let s: string = i.toString();
  total = total + s.length;
}
console.log(total);
