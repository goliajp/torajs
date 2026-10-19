// a call between the null guard and the use can store null into the
// guarded binding; the read inside the callee throws like bun does
type Node = { n: number };
let q: Node | null = { n: 1 };
function f(): boolean { q = null; return true }
function g(x: Node): number { return x.n }
try {
  if (q !== null && f()) console.log(g(q));
} catch (e) {
  console.log((e as Error).constructor.name);
}
let p: Node | null = { n: 2 };
if (p !== null) { f(); try { console.log(g(p)) } catch (e) { console.log("caught") } }
console.log(q === null, p !== null);

// the call sits in the guarded body, not in the condition
let r: Node | null = { n: 3 };
function wr(): void { r = null }
if (r !== null) {
  console.log(g(r));
  wr();
  try { console.log(g(r)) } catch (e) { console.log("r", (e as Error).constructor.name) }
}

// the store happens in a function the callee calls
let s: Node | null = { n: 4 };
function clear_s(): void { s = null }
function step(): number { clear_s(); return 1 }
if (s !== null) {
  step();
  try { console.log(g(s)) } catch (e) { console.log("s", (e as Error).constructor.name) }
}

// the store is a closure's, into a local it captured
function k(): number {
  let t: Node | null = { n: 5 };
  const drop = () => { t = null };
  if (t !== null) {
    drop();
    return g(t);
  }
  return 0;
}
try { console.log(k()) } catch (e) { console.log("k", (e as Error).constructor.name) }

// a narrow from a store, then a call that nulls it
let w: Node | null = { n: 6 };
function ww(): void { w = null }
w = { n: 7 };
ww();
try { console.log(g(w)) } catch (e) { console.log("w", (e as Error).constructor.name) }

// a narrow from an early exit, then a loop whose call nulls it
let u: Node | null = { n: 10 };
function wu(): void { u = null }
if (u === null) throw new Error("unreachable");
for (let i = 0; i < 2; i++) {
  try { console.log(g(u)) } catch (e) { console.log("u", (e as Error).constructor.name) }
  wu();
}

// a callee that stores into another name leaves the narrow alone
let v: Node | null = { n: 8 };
let other: Node | null = { n: 9 };
function wo(): void { other = null }
if (v !== null) {
  wo();
  console.log(g(v));
}
console.log(other === null);
