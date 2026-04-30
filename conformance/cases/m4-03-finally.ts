function f(n: number): number {
  let r: number = 0;
  try {
    if (n < 0) { throw 99; }
    r = n + 1;
  } catch (e) {
    r = e + 1000;
  } finally {
    console.log(r);
  }
  return r;
}
console.log(f(7));
console.log(f(-5));
