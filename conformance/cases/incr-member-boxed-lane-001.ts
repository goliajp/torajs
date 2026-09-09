// `o.k++` on a receiver the incr ladder has no arm for was a hard
// panic — "not yet supported: post-incr field on non-obj <shape>" —
// for Map, Date, Str and every primitive repr, while the SAME program
// through an `any` binding ran and gave the right answer for each.
// So the ladder now ends in the boxed any lane, the last lane the
// STORE ladder grew in rotation 527's write-side twin. The lane
// dispatches on the runtime tag, which is what makes one arm able to
// answer all of these: an extensible object takes the expando write,
// a primitive raises the TypeError §10.1.8.1 owes it.
function show(tag: string, f: () => void) {
  try {
    f();
    console.log(tag, "ok");
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
const m = new Map();
(m as any).zz = 0;
show("map", () => {
  m.size;
  (m as any).zz++;
});
console.log("map zz =", (m as any).zz);

const d = new Date();
(d as any).zz = 0;
show("date", () => {
  (d as any).zz++;
});
console.log("date zz =", (d as any).zz);

const s = "ab";
show("str", () => {
  (s as any).zz++;
});

// A specialized body's parameter — the receiver repr the checker never
// spelled `any`, and the one that surfaced this.
function f(n) {
  n.zz++;
  return "after";
}
show("num-param", () => {
  console.log(f(1));
});
show("bool-param", () => {
  console.log(f(true));
});
