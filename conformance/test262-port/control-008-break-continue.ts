// Adapted from test262: language/statements/break + continue/*.js
// `break` exits the innermost loop; `continue` jumps to its step/header.
function check(): number {
  let breakAt: number = -1;
  for (let i: number = 0; i < 100; i = i + 1) {
    if (i === 7) { breakAt = i; break; }
  }
  if (breakAt !== 7) { throw "#1"; }

  let evenCount: number = 0;
  for (let i: number = 0; i < 10; i = i + 1) {
    if (i % 2 !== 0) { continue; }
    evenCount = evenCount + 1;
  }
  if (evenCount !== 5) { throw "#2"; }

  return 0;
}
console.log(check());
