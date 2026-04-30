// Adapted from test262: pair of independent stateful closures sharing
// the same constructor — verifies each closure has its OWN env block.
type S = { n: number };

function makeAdd(): (delta: number) => number {
  let s: S = { n: 0 };
  return (delta: number): number => {
    s.n = s.n + delta;
    return s.n;
  };
}

function check(): number {
  let a = makeAdd();
  let b = makeAdd();
  if (a(5) !== 5) { throw "#1"; }
  if (b(10) !== 10) { throw "#2"; }
  if (a(3) !== 8) { throw "#3"; }
  if (b(2) !== 12) { throw "#4"; }
  if (a(0) !== 8) { throw "#5: a unaffected by b"; }
  if (b(0) !== 12) { throw "#6: b unaffected by a"; }
  return 0;
}
console.log(check());
