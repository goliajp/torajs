// Adapted from test262: language/statements/do-while/* — body always
// runs at least once; cond decides whether to repeat.
function check(): number {
  // Body executes once even when cond is false from the start.
  let n: number = 0;
  do { n++; } while (n < 1);
  if (n !== 1) { throw "#1: cond-false-from-start"; }

  // Standard loop counting.
  let m: number = 0;
  do { m += 2; } while (m < 10);
  if (m !== 10) { throw "#2"; }

  // break inside do-while.
  let i: number = 0;
  do {
    i++;
    if (i === 3) { break; }
  } while (i < 100);
  if (i !== 3) { throw "#3"; }

  // continue inside do-while.
  let count: number = 0;
  let j: number = 0;
  do {
    j++;
    if (j % 2 === 0) { continue; }
    count++;
  } while (j < 6);
  // odd values 1,3,5 → 3 increments
  if (count !== 3) { throw "#4"; }
  return 0;
}
console.log(check());
