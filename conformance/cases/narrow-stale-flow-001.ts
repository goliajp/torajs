// a value read off a narrow a call may have undone is checked wherever
// it travels: through another parameter, a return, a field, an alias,
// a container; a read of a member of null throws, the value itself
// passes through untouched
type Node = { n: number };
let q: Node | null = { n: 1 };
function f(): boolean { q = null; return true }
function reset(): void { q = { n: 1 } }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  reset();
}

function inner(y: Node): number { return y.n }
function outer(z: Node): number { return inner(z) }
run("param", () => { if (q !== null && f()) console.log(outer(q)) });

function id(w: Node): Node { return w }
run("return", () => { if (q !== null && f()) console.log(id(q).n) });

class Box { v: Node; constructor(v: Node) { this.v = v } }
run("ctor", () => { if (q !== null && f()) console.log(new Box(q).v.n) });

run("alias", () => { if (q !== null) { f(); const y: Node = q; console.log(outer(y)) } });

run("literal", () => { if (q !== null && f()) { const o = { v: q }; console.log(o.v.n) } });

run("capture", () => {
  if (q !== null && f()) { const y = q; const k = (): number => y.n; console.log(k()) }
});

const xs: Node[] = [];
run("push", () => { if (q !== null && f()) { xs.push(q); for (const e of xs) console.log(e.n) } });

const m = new Map<string, Node>();
run("map", () => {
  if (q !== null && f()) { m.set("a", q); const e = m.get("a"); if (e !== undefined) console.log(e.n) }
});

function idg<T>(x: T): T { return x }
run("generic", () => { if (q !== null && f()) console.log(idg(q).n) });

// passed along without a member read, the value is null
function pass(x: Node): Node { return x }
run("pass", () => { if (q !== null && f()) { const z = pass(q); console.log(z === null, z) } });
run("print", () => { if (q !== null && f()) console.log(q, typeof q, `${q}`, String(q)) });
run("optional", () => { if (q !== null && f()) console.log(q?.n, (q ?? { n: 9 }).n) });

// a callee storing into another binding of the same name leaves this one
function own(): number {
  let q: Node | null = { n: 7 };
  if (q !== null) { f(); return q.n }
  return 0;
}
run("own", () => console.log(own()));
