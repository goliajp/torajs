// Adapted from test262: language/statements/while/*.js — basic form.
function check(): number {
  let n: number = 10;
  let p: number = 1;
  while (n > 0) {
    p = p * 2;
    n = n - 1;
  }
  if (p !== 1024) { throw "#1: 2^10 = 1024"; }
  return 0;
}
console.log(check());
