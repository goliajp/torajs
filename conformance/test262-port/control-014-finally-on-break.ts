// Adapted from test262: try-finally where the try-body breaks out of an
// enclosing loop. Spec: finally runs before the break takes effect.
function f(): number {
  let log: number = 0;
  for (let i: number = 0; i < 10; i = i + 1) {
    try {
      if (i === 3) { break; }
      log = log + i;
    } finally {
      log = log + 100;   // adds 100 each iter, including the break iter
    }
  }
  return log;
}

function check(): number {
  // i=0: try adds 0, finally adds 100 → 100
  // i=1: try adds 1, finally adds 100 → 201
  // i=2: try adds 2, finally adds 100 → 303
  // i=3: try sees break, finally adds 100 → 403, then break exits loop
  if (f() !== 403) { throw "#1"; }
  return 0;
}
console.log(check());
