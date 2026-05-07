async function double(v: number): number {
  return v + v;
}

let total: number = 0;
for (let i: number = 0; i < 100000; i = i + 1) {
  total = total + (await double(i));
}
console.log(total);
