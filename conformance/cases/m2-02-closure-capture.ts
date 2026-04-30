function makeAdder(n: number): number {
  let f = (x: number): number => x + n;
  return f(10);
}
console.log(makeAdder(5));
console.log(makeAdder(7));
