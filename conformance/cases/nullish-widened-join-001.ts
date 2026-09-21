// §13.4.2 puts no condition on the two sides of `??`: it picks one
// and the result is the union. This lattice has no general union,
// only `Nullable(T)`, and every pair it could not spell was REFUSED
// — including `o ?? "dflt"` on `o: { a: number } | undefined`, the
// most ordinary use of the operator that type exists for.

// --- the shape that was refused, over the pointer families ---
const o: { a: number } | undefined = undefined;
console.log(o ?? "dflt");

const xs: number[] | undefined = undefined;
console.log(xs ?? "no array");

class C {
  a = 1;
}
const c: C | undefined = undefined;
console.log(c ?? "no instance");

// ... and the same slots holding a live value take the lhs
const o2: { a: number } | undefined = { a: 7 };
console.log((o2 ?? "dflt") === o2);

const xs2: number[] | undefined = [1, 2];
console.log(xs2 ?? "no array");

// --- a null / undefined rhs is a union this lattice CAN spell
// (`T | null`), so it stays on the pointer slot rather than widening
console.log(o ?? null);
console.log(o ?? undefined);
console.log(o2 ?? null);

// --- the scalar and string families answer through the same
// re-encoding on the way into the box
const n: number | undefined = undefined;
console.log(n ?? "no number");
const s: string | undefined = undefined;
console.log(s ?? 7);
const s2: string | undefined = "live";
console.log(s2 ?? 7);

// --- §13.4.2: the rhs is NOT evaluated when the lhs is not nullish
let ticks = 0;
function loud(): string {
  ticks = ticks + 1;
  return "loud";
}
console.log(o2 ?? loud());
console.log(ticks);
console.log(o ?? loud());
console.log(ticks);

// --- the pairs that already met in the lattice keep their own lanes
const m: { a: number } | undefined = undefined;
console.log((m ?? { a: 3 }).a);
const q: number | undefined = undefined;
console.log(q ?? 5);
const t: string | undefined = undefined;
console.log(t ?? "s");
const any1: any = undefined;
console.log(any1 ?? "from any");
console.log(1 ?? "unreached");
console.log(null ?? "from null");
