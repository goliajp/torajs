// a narrowed binding a call set to null, read again through a
// parameter, a local and a call result whose type is not nullable: the
// value passes along as it is, and the member read off it throws a
// TypeError
type Node = { n: number };
let q: Node | null = { n: 2 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 2 };
}

function read(x: Node): number { return x.n }
function twice(x: Node): number { return read(x) + read(x) }
function keep(x: Node): Node { return x }
function wrap(x: Node): Node[] { return [x] }
const pick = (): Node => {
  let r: Node = { n: 0 };
  if (q !== null && nullQ()) r = q;
  return r;
};

// through a parameter, one or two calls deep
run("param", () => { if (q !== null && nullQ()) console.log(read(q)) });
run("param2", () => { if (q !== null && nullQ()) console.log(twice(q)) });

// through a local, declared or reassigned after a first live read
run("const", () => { if (q !== null && nullQ()) { const z: Node = q; console.log(z.n) } });
run("let", () => {
  if (q !== null) { let z: Node = q; console.log(z.n); if (q !== null && nullQ()) z = q; console.log(z.n) }
});

// through a call result: a declared function, a closure, an element
run("ret", () => { if (q !== null && nullQ()) console.log(keep(q).n) });
run("closure", () => console.log(pick().n));
run("nested", () => console.log(read(keep(pick()))));
run("element", () => { if (q !== null && nullQ()) console.log(wrap(q)[0].n) });

// passed along without a member read, the value is null
run("pass", () => { const z = keep(pick()); console.log(z === null, z, typeof z) });

// the live path still reads
const live: Node = { n: 7 };
console.log(read(live), twice(live), keep(live).n, read(keep({ n: 1 })));
