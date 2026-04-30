// Adapted from TS generic struct — `Pair<A, B>` instantiates a fresh
// concrete struct layout per (A, B). Field reads go through the
// monomorphized layout, no boxing.
type Pair<A, B> = { fst: A, snd: B };

function check(): number {
  let p1: Pair<number, number> = { fst: 7, snd: 9 };
  if (p1.fst !== 7) { throw "#1"; }
  if (p1.snd !== 9) { throw "#2"; }
  if (p1.fst + p1.snd !== 16) { throw "#3"; }

  let p2: Pair<number, number> = { fst: 100, snd: 200 };
  if (p2.fst + p2.snd !== 300) { throw "#4"; }
  return 0;
}
console.log(check());
