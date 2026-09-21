// `any | undefined` and `x?: any` denote `any`, not a narrower type.
//
// TypeScript folds a union with `any` into `any` (tsc --strict: under
// `type X = any | undefined`, `0 extends (1 & X)` is true). tr used to
// build `Nullable(Any)` for these spellings, and that type had no arm
// in the member lane — so every member read and every method call on
// such a slot was REFUSED at compile time, on programs bun runs. The
// any lane, which exists so an unknown shape still compiles, was
// unreachable through this spelling.
//
// Written out directly, not through a helper: a closure or a parameter
// does not carry the receiver's annotation, so a wrapper would resolve
// the member against a different type and hide the entry (r687).

function pickArr(): any | undefined {
  return Math.random() < 2 ? [1, 2, 3] : undefined;
}
function pickNone(): any | undefined {
  return undefined;
}
function takesOptional(x?: any): number {
  return x === undefined ? -1 : x.length;
}

function main(): void {
  const live: any | undefined = pickArr();
  console.log(live.length);
  console.log(live.join("-"));
  console.log(live.join());
  console.log(live[0]);

  // Same slot, statically known to hold undefined: the any lane must
  // reach the spec's runtime TypeError (§6.2.5.5 -> §7.1.18 ToObject),
  // not a compile-time refusal.
  const gone: any | undefined = pickNone();
  try {
    console.log(gone.length);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }

  console.log(takesOptional(), takesOptional([7, 8]));
}

main();
