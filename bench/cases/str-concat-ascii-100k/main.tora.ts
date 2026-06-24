let prefix: string = "  Hello42 word  ";
let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let s: string = prefix + "x";
  total = total + s.length;
}
console.log(total);
