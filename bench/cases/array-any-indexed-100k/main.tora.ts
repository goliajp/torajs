let arr: string[] = [];
for (let j: number = 0; j < 100; j = j + 1) arr.push("item" + j.toString());
let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let s: string = arr[i % 100];
  total = total + s.length;
}
console.log(total);
