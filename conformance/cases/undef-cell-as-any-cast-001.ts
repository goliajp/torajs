// `x as any` is a pass-through for a heap value: the operand keeps
// its static SSA type and the any lane boxes it wherever it needs
// to. That is right until the static type is the only thing that
// knows this slot's `undefined` is an in-band address — after the
// cast, the type says `any` and the source is an `As`, so nothing
// downstream can ask the question any more.
//
// So the cast re-encodes when both halves of the question are still
// in hand. Six surfaces were wrong in both spellings, and in both
// directions: `"x" + (r as any)` and the `in` / hasOwn pair read the
// immortal cell as a live object, while `(r as any) * 2` called it.

const xs: number[][] = [];
const r = xs.find((z: number[]) => z.length > 99);
const a: number[] | undefined = undefined;

// Written flat on purpose: a closure capture does not carry the
// fact that a slot may be holding the cell, so a `say(label, () =>
// …)` helper would hide the entry it is meant to test.

// --- §7.1.4 ToNumber(undefined) is NaN, not a call of the receiver
console.log((r as any) * 2, +(r as any), (a as any) * 2);
// --- §7.1.1 ToPrimitive then §13.15.3: string concatenation
console.log("x" + (r as any), "x" + (a as any));
// --- §13.10.1 step 5 / §20.1.2.13 step 1: both want an Object
try {
  console.log("in-op", "ok", "a" in (r as any));
} catch (e) {
  console.log("in-op", "threw", (e as Error).constructor.name);
}
try {
  console.log("has-own", "ok", Object.hasOwn(r as any, "a"));
} catch (e) {
  console.log("has-own", "threw", (e as Error).constructor.name);
}
try {
  console.log("in-op-annotated", "ok", "a" in (a as any));
} catch (e) {
  console.log("in-op-annotated", "threw", (e as Error).constructor.name);
}
try {
  console.log("has-own-annotated", "ok", Object.hasOwn(a as any, "a"));
} catch (e) {
  console.log("has-own-annotated", "threw", (e as Error).constructor.name);
}
// --- §7.2.13 relational: undefined compares false
console.log((r as any) < 1, (r as any) > 1);
// --- and the cast agrees with the plain `any` binding it is the
// cast form of
const b: any = r;
console.log(typeof (r as any), typeof b, (r as any) === undefined);

// --- a live value keeps every answer the pass-through gave it
const live = [[1, 2], [3]];
const hit = live.find((z: number[]) => z.length > 1);
console.log(typeof (hit as any), (hit as any).length, (hit as any).join("-"));
console.log("y" + (hit as any), "a" in (hit as any), 1 in (hit as any));
console.log([...(hit as any)].join("-"), Array.from(hit as any).join("-"));
const liveA: number[] | undefined = [7];
console.log("z" + (liveA as any), (liveA as any).length);

// --- the families that never had a cell keep theirs
console.log(typeof (7 as any), typeof ("s" as any), typeof (true as any));
console.log(typeof (undefined as any), typeof (null as any));
console.log((undefined as any) === undefined, (null as any) === null);
