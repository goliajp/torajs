// A typed catch binding of `null`, `undefined`, or a function type
// with `null` / `undefined` beside it, admits that value (and, for the
// function type, any function object) and binds it in an `any` slot.
//
// `catch (e: T)` with a T other than `any` / `unknown` is no
// TypeScript (TS1196); tr takes the annotation as the slot's type and
// raises a TypeError where the binding is made for a value the type
// does not describe, as for every typed catch. bun strips the
// annotation and binds whatever was thrown, so the `.expected` lines
// that read "rejected" differ from bun: "number into null", "string
// into undefined", "object into null", "number into nullable fn" and
// "string into nullable fn" (bun prints the value the body reads), and
// the loop's line (bun: rejected 0).
// The loop throws a heap object into `e: null` many times: each
// rejected value is released before the TypeError leaves.

function bind(label: string, f: () => string): void {
  try {
    console.log(label, "bound", f());
  } catch (err) {
    console.log(label, "rejected", err instanceof TypeError, (err as Error).message);
  }
}

class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

const seven = (): number => 7;
const xs: any[] = [null, undefined];

bind("null into null", () => {
  try { throw null } catch (e: null) { return String(e === null) + " " + typeof e }
});
bind("any null into null", () => {
  try { throw xs[0] } catch (e: null) { return String(e) }
});
bind("number into null", () => {
  try { throw 5 } catch (e: null) { return String(e) }
});
bind("object into null", () => {
  try { throw new Box(3) } catch (e: null) { return String(e) }
});
bind("undefined into undefined", () => {
  try { throw undefined } catch (e: undefined) { return String(e === undefined) + " " + typeof e }
});
bind("string into undefined", () => {
  try { throw "s" } catch (e: undefined) { return String(e) }
});
bind("null into nullable fn", () => {
  try { throw null } catch (e: (() => number) | null) { return String(e === null) }
});
bind("undefined into nullable fn", () => {
  try { throw xs[1] } catch (e: (() => number) | undefined) { return String(e === undefined) + " " + typeof e }
});
bind("closure into nullable fn", () => {
  try { throw seven } catch (e: (() => number) | null) { return String(e !== null ? e() : -1) }
});
bind("closure into optional fn", () => {
  try { throw seven } catch (e: (() => number) | undefined) { return typeof e + " " + (e ? e() : -1) }
});
bind("number into nullable fn", () => {
  try { throw 5 } catch (e: (() => number) | undefined) { return typeof e }
});
bind("string into nullable fn", () => {
  try { throw "x" } catch (e: (() => number) | null) { return typeof e }
});

let rejected = 0;
let bound = 0;
for (let i = 0; i < 20000; i++) {
  try {
    try { throw new Box(i) } catch (e: null) { if (e === null) bound++ }
  } catch (err) {
    if (err instanceof TypeError) rejected++;
  }
}
console.log("loop rejected", rejected, "bound", bound);
