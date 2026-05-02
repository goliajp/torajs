let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let parts: string[] = "3 4 + 2 * 5 +".split(" ");
  total = total + parts.length;
}
console.log(total);
