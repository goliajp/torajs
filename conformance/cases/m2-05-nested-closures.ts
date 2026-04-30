function outer(a: number): number {
  let inner = (b: number): number => {
    let inner2 = (c: number): number => a + b + c;
    return inner2(100);
  };
  return inner(10);
}
console.log(outer(1));
