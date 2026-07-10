// heap sources (Str / Arr / Obj) into a declared-`any` struct field:
// objlit init, member re-assign, any->typed read-back, and anon-twin
// coexistence. Archived as a chunk-757 suspect (raw store / twin
// layout misdecode); verified closed by the C4 admit+box lane +
// the chunk-780 declared-layout hint — this fixture pins it.
type A = { v: any };

const o: A = { v: "s" };
console.log(o.v);
const p: A = { v: [1, 2] };
console.log(p.v);
const q: A = { v: { x: 1 } };
console.log(q.v);

const m: A = { v: 1 };
const s0 = "he" + "llo";
m.v = s0;
console.log(m.v);
m.v = [3, 4];
console.log(m.v);

function run(): void {
  const o2: A = { v: "hello" };
  const s: string = o2.v;
  console.log(s.length, s + "!");
  const raw = { v: "t" };
  console.log(o2.v, raw.v);
  const arr: A = { v: [1, 2, 3] };
  const xs: number[] = arr.v;
  console.log(xs.length, xs[2]);
}
run();
