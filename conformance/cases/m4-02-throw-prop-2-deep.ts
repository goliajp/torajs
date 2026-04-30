function inner(n: number): number {
  if (n === 0) { throw 42; }
  return n * 2;
}
function middle(n: number): number {
  return inner(n) + 1;
}
function outer(n: number): number {
  try { return middle(n); } catch (e) { return e + 1000; }
}
console.log(outer(5));
console.log(outer(0));
