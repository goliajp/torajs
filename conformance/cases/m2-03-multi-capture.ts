function makeF(a: number, b: number): number {
  let f = (x: number): number => x * a + b;
  return f(10);
}
console.log(makeF(3, 7));
console.log(makeF(11, 2));
