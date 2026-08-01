// §7.3.25 CopyDataProperties — `{ ...src }` with a runtime-shaped
// (any) source runs the own-enumerable walk into a dynobj-lane
// literal instead of the compile-time struct unfold.
const a: any = { x: 1, y: 2 };
const b = { ...a, z: 3 };
console.log(b.x, b.y, b.z);

// inline members win on key collision; spread order is write order
const c = { x: 9, ...a };
console.log(c.x, c.y);
const d = { ...a, x: 7 };
console.log(d.x);

// nullish sources contribute nothing
const e = { ...(null as any), k: 1 };
console.log(e.k);
const f = { ...(undefined as any) };
console.log(JSON.stringify(f));

// two any spreads, last wins
const g: any = { x: 10, w: 4 };
const h = { ...a, ...g };
console.log(h.x, h.y, h.w);

// string primitive source spreads index fields
const s = { ...("ab" as any) };
console.log((s as any)[0], (s as any)[1]);

// number primitive source spreads nothing
const n = { ...(5 as any), ok: true };
console.log(JSON.stringify(n));

// getter on the source runs through [[Get]]
const src: any = {};
Object.defineProperty(src, "gv", {
  get() {
    return 42;
  },
  enumerable: true,
});
const gc = { ...src };
console.log(gc.gv);

// non-enumerable keys are skipped
Object.defineProperty(src, "hidden", { value: 1, enumerable: false });
const ge = { ...src };
console.log((ge as any).hidden);

// spread product feeds normal member ops
const m = { ...a };
(m as any).extra = "e";
console.log((m as any).extra, Object.keys(m).length);
