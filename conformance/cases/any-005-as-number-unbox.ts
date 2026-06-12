// b1 — `<Any-valued> as number` must unbox (spec ToNumber on the
// boxed value), and Map's value slot must answer width queries so
// fractional values survive the let-slot face.
// (A user class declaring its own `get` method is kept out of this
// fixture: name-based class-method dispatch in check.rs currently
// false-rejects unrelated Map.get receivers — pre-existing,
// tracked in plan-state L3b.)

// fract value through Map.get + as number
let m = new Map<number, number>();
m.set(1, 2.5);
let n: number = m.get(1) as number;
console.log(n);

// int value stays exact
let mi = new Map<number, number>();
mi.set(1, 7);
let ni: number = mi.get(1) as number;
console.log(ni);

// string value passes through untouched
let ms = new Map<number, string>();
ms.set(1, "hi");
let sv: string = ms.get(1) as string;
console.log(sv);

// JSON.parse scalar into a number slot (the `JSON.parse(..) as
// number` spelling is a loud not-yet-supported member-call shape,
// not a silent face - LetDecl-ann is the supported form)
let jx: number = JSON.parse("2.5");
console.log(jx);

// direct read (no as) keeps working
console.log(m.get(1));

// Set.add value width flows the same write edge
let st = new Set<number>();
st.add(1.5);
for (const v of st) {
  console.log(v);
}
