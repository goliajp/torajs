let total: number = 0;
let piece: string = "中文测试";
let n: number = 1000;
for (let i: number = 0; i < n; i = i + 1) {
  let acc: string = "";
  for (let j: number = 0; j < 100; j = j + 1) {
    acc = acc + piece;
  }
  total = total + acc.length;
}
console.log(total);
