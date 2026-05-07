function add1(v: number): number { return v + 1; }

let total: number = 0;
for (let i: number = 0; i < 100000; i = i + 1) {
  total = total + (await Promise.resolve(i).then(add1));
}
console.log(total);
