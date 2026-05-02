let q: number[] = [];
let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  q.push(i);
  if (q.length > 16) {
    total = total + q.shift();
  }
}
while (q.length > 0) {
  total = total + q.shift();
}
console.log(total);
