function f(): number {
  let total: number = 0;
  for (let i: number = 0; i < 5; i = i + 1) {
    let inner: number = i * 10;
    total = total + inner;
  }
  return total;
}
console.log(f());
