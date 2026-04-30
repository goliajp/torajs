// Integration: closure capturing a counter via wrapping in a struct
// (since tr's value-shape number capture can't be mutated).
type Cnt = { n: number };

function makeCounter(): (delta: number) => number {
  let state: Cnt = { n: 0 };
  return (delta: number): number => {
    state.n = state.n + delta;
    return state.n;
  };
}

function check(): number {
  let c = makeCounter();
  if (c(1) !== 1) { throw "#1"; }
  if (c(2) !== 3) { throw "#2"; }
  if (c(-1) !== 2) { throw "#3"; }
  if (c(10) !== 12) { throw "#4"; }
  // Independent counter, doesn't share state.
  let d = makeCounter();
  if (d(5) !== 5) { throw "#5"; }
  if (c(0) !== 12) { throw "#6"; }
  return 0;
}
console.log(check());
