// Adapted from test262: language/statements/for/*.js — basic form.
function check(): number {
  let s: number = 0;
  for (let i: number = 0; i < 10; i = i + 1) {
    s = s + i;
  }
  if (s !== 45) { throw "#1: 0+1+...+9 = 45"; }
  let n: number = 0;
  for (let i: number = 0; i < 100; i = i + 1) {
    if (i === 50) { break; }
    n = n + 1;
  }
  if (n !== 50) { throw "#2: break exits loop"; }
  let c: number = 0;
  for (let i: number = 0; i < 10; i = i + 1) {
    if (i % 2 === 0) { continue; }
    c = c + 1;
  }
  if (c !== 5) { throw "#3: continue skips iter"; }
  return 0;
}
console.log(check());
