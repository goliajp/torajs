// Adapted from test262: language/expressions/optional-chaining/* —
// `obj?.field`. Null short-circuits; otherwise reads the field.
//
// Caveat vs JS spec: JS uses `undefined` as the short-circuit value
// (`null?.x === undefined`); tr only has `null` in this subset, so
// `null?.x === null`. The behavior is observable when comparing
// against null literally — we route around it by composing with `??`,
// which yields the same final values either way.
type Pt = { x: number, y: number };

function check(): number {
  let q: Pt | null = { x: 7, y: 9 };
  let p: Pt | null = null;

  // Non-null path: returns the field. Compose with `??` to land on a
  // non-nullable value the test can verify.
  let xv = q?.x ?? -1;
  if (xv !== 7) { throw "#1"; }

  // Null path: ?. yields null/undefined; ?? supplies the fallback.
  let xnull = p?.x ?? 999;
  if (xnull !== 999) { throw "#2"; }

  return 0;
}
console.log(check());
