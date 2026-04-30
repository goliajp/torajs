// Adapted from test262: generic with two type params, both used at
// distinct positions in a struct + accessor pattern.
type Pair<A, B> = { fst: A, snd: B };

function makeP<A, B>(a: A, b: B): Pair<A, B> {
  return { fst: a, snd: b };
}

function check(): number {
  let p1 = makeP(1, "x");
  if (p1.fst !== 1) { throw "#1"; }
  if (p1.snd !== "x") { throw "#2"; }
  let p2 = makeP(true, 42);
  if (p2.fst !== true) { throw "#3"; }
  if (p2.snd !== 42) { throw "#4"; }
  return 0;
}
console.log(check());
