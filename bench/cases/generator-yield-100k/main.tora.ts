function* range(start: number, end: number): Generator<number> {
  let i: number = start;
  while (i < end) {
    yield i;
    i = i + 1;
  }
}

let total: number = 0;
let passes: number = 100;
for (let p: number = 0; p < passes; p = p + 1) {
  for (let v of range(0, 1000)) {
    total = total + v;
  }
}
console.log(total);
