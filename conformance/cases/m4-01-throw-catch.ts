function check(n: number): number {
  if (n < 0) { throw 99; }
  return n + 1;
}
function safe(n: number): number {
  try { return check(n); } catch (e) { return 0; }
}
console.log(safe(5));
console.log(safe(-5));
