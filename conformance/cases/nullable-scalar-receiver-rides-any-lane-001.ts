// A scalar's nullable already lives in the NaN-boxed any lane, so its
// member read has an answer — the any lane's.
//
// `number | undefined` and `boolean | undefined` do not get a raw
// slot: the box tax their slot pays is exactly what lets them hold
// undefined (`rides_any_lane`). The operand lanes have asked that
// question since they were written — `n + 1` on such a slot answers
// NaN per §13.15.3 — but the member lane never did, so `n.toFixed(2)`
// was refused on the same binding. The spec draws no such distinction:
// §13.3.4 builds the Reference Record either way and §7.1.20 ToObject
// raises the TypeError at run time if the value is absent.
//
// Written out directly, never through a helper (r687).

function maybeNum(): number | undefined {
  return Math.random() < 2 ? 1.5 : undefined;
}
function noNum(): number | undefined {
  return undefined;
}
function maybeBool(): boolean | undefined {
  return Math.random() < 2 ? true : undefined;
}

function main(): void {
  const n: number | undefined = maybeNum();
  console.log(n.toFixed(2));
  console.log(n.toString());

  const b: boolean | undefined = maybeBool();
  console.log(b.toString());

  // The operand lane on the same slot, unchanged.
  console.log((n as number) + 1);

  const gone: number | undefined = noNum();
  try {
    console.log(gone.toFixed(2));
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
}

main();
