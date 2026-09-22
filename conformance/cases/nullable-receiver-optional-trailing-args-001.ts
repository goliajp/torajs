// Methods with optional trailing parameters work on a possibly-absent
// receiver: `r.join()`, `r.slice()`, `r.sort()`, `r.fill(v)`,
// `r.copyWithin(t, s)`, `s.slice(1)`, `s.substring(1)`, `s.search(re)`.
//
// The typed wedges that implement these shapes each destructure the
// receiver's type, so a `Nullable` receiver made every one of them
// decline, and the call fell through to the member table — whose
// signatures spell the optional parameters as required. `r.join("-")`
// passed while `r.join()` failed "expected 1 argument(s), got 0". The
// wedges now read the receiver the way the member lane does; the type
// recorded for the receiver still says Nullable, so the lowering's
// guard still raises the TypeError for an absent one.
//
// Written out directly, never through a helper (r687).

function maybeA(): number[] | undefined {
  return Math.random() < 2 ? [3, 1, 2] : undefined;
}
function noneA(): number[] | undefined {
  return undefined;
}
function maybeS(): string | undefined {
  return Math.random() < 2 ? "abc" : undefined;
}

function main(): void {
  const a: number[] | undefined = maybeA();
  console.log(a.join(), a.slice().length, a.slice(1).length);
  console.log(a.sort().join("-"));
  console.log(a.fill(7, 1).join(), a.copyWithin(0, 1).join());

  const s: string | undefined = maybeS();
  console.log(s.slice(1), s.substring(1), s.search(/c/));

  const g: number[] | undefined = noneA();
  try {
    console.log(g.join());
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  try {
    console.log(g.sort());
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
}

main();
