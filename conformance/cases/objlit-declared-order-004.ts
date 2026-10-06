// literals spelled in another order than their declared struct type in
// array elements, nullable slots, explicit type arguments and yields:
// reads use the declared layout, enumeration follows creation order
type A = { b: string; x: number };

function show(tag: string, a: A): void {
  console.log(tag, a.b, a.x);
  console.log(JSON.stringify(a), Object.keys(a).join(","));
  const ks: string[] = [];
  for (const k in a) ks.push(k);
  console.log(ks.join(","), JSON.stringify({ ...a }));
  console.log({ ...a });
}

const base: A = { b: "k", x: 0 };
const xs: A[] = [{ x: 1, b: "q" }, { x: 2, b: "r" }];
console.log(xs[1].b, xs[0].x, JSON.stringify(xs));
show("arr0", xs[0]);
const mixed: A[] = [base, { x: 3, b: "m" }, { b: "o", x: 4 }];
console.log(JSON.stringify(mixed));
for (const a of mixed) show("mixed", a);
const spread: A[] = [...xs, { x: 5, b: "s" }];
show("spread", spread[2]);
const deep: A[][] = [[{ x: 6, b: "d" }], []];
show("deep", deep[0][0]);

let n: A | null = null;
n = { x: 7, b: "n" };
console.log("n", JSON.stringify(n));
if (n) show("null", n);
const m: A | null = { x: 8, b: "c" };
if (m) show("nullc", m);
let u: A | undefined = undefined;
u = { x: 9, b: "u" };
if (u) show("undef", u);

type W = { a?: A; k: number };
const w: W = { k: 1, a: { x: 10, b: "w" } };
console.log(JSON.stringify(w));
if (w.a) show("opt", w.a);
type H = { as: A[]; o: A | null };
const h: H = { o: { x: 11, b: "o" }, as: [{ x: 12, b: "h" }] };
console.log(JSON.stringify(h));
show("field-arr", h.as[0]);

const pushed: A[] = [];
pushed.push({ x: 13, b: "p" });
pushed.push({ x: 14, b: "p2" }, { b: "p3", x: 15 });
console.log(JSON.stringify(pushed));
show("push", pushed[1]);

function all(list: A[]): string {
  return list.map((a) => a.b + a.x).join(",");
}
console.log(all([{ x: 16, b: "g" }, { x: 17, b: "h" }]));
function mkList(): A[] {
  return [{ x: 18, b: "r" }];
}
show("ret-arr", mkList()[0]);
function mkNull(): A | null {
  return { x: 19, b: "rn" };
}
const rn = mkNull();
if (rn) show("ret-null", rn);

function id<T>(v: T): T {
  return v;
}
show("generic", id<A>({ x: 20, b: "i" }));
function pair<T>(a: T, b: T): T[] {
  return [a, b];
}
const ps = pair<A>({ x: 21, b: "p" }, { b: "q", x: 22 });
show("pair0", ps[0]);
show("pair1", ps[1]);

function* gen(): Generator<A> {
  yield { x: 23, b: "y" };
  yield { b: "z", x: 24 };
}
for (const a of gen()) show("yield", a);
function* genList(): Generator<A[]> {
  yield [{ x: 25, b: "yl" }];
}
for (const l of genList()) show("yield-arr", l[0]);

const withNull: (A | null)[] = [{ x: 26, b: "wn" }, null, { b: "wo", x: 27 }];
console.log(JSON.stringify(withNull));
for (const a of withNull) {
  if (a) show("with-null", a);
  else console.log("null");
}
const withUndef: (A | undefined)[] = [undefined, { x: 28, b: "wu" }];
console.log(JSON.stringify(withUndef));
const wu = withUndef[1];
if (wu) show("with-undef", wu);

function* genNull(): Generator<A | null> {
  yield { x: 29, b: "gn" };
  yield null;
  yield { b: "go", x: 30 };
}
for (const a of genNull()) {
  if (a) show("yield-null", a);
  else console.log("null");
}
function* genObj(): Generator<object> {
  yield { x: 31, b: "obj" };
  yield { k: 32 };
}
for (const o of genObj()) console.log(JSON.stringify(o), Object.keys(o).join(","));
type Wrap = { a: A; k: number };
function* genNested(): Generator<Wrap> {
  yield { a: { b: "in", x: 33 }, k: 1 };
  yield { k: 2, a: { x: 34, b: "re" } };
}
for (const w of genNested()) {
  show("yield-nested", w.a);
  console.log(JSON.stringify(w), Object.keys(w).join(","));
}
