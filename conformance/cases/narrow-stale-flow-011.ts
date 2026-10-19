// a narrowed binding a call set to null, handed to a function that
// reads a member of it: the TypeError comes when the read would run,
// after whatever the callee does first, and a specialization of a
// generic function reads it the same way
type Node = { n: number; next: Node | null };
let q: Node | null = { n: 1, next: null };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1, next: null };
}

// the read is the first thing the callee does
function first(x: Node): number { return x.n }
function firstIf(x: Node, k: number): number {
  const base = 10;
  let acc = 0;
  if (x.n > k) acc = base;
  return acc + x.n;
}
function loop(x: Node): number {
  let t = 0;
  for (let i = 0; i < x.n; i++) t += i;
  return t;
}
// something the caller can see happens before the read
function after(x: Node): number { console.log("entered"); return x.n }
function afterArg(x: Node, y: number): number { return y + x.n }
let ticks = 0;
function tick(): number { ticks++; return ticks }
// a specialization reads the parameter of its generic body
function rd<T extends { n: number }>(x: T): number { return x.n }

run("first", () => { if (q !== null && nullQ()) console.log(first(q)) });
run("if", () => { if (q !== null && nullQ()) console.log(firstIf(q, 0)) });
run("loop", () => { if (q !== null && nullQ()) console.log(loop(q)) });
run("after", () => { if (q !== null && nullQ()) console.log(after(q)) });
run("args", () => { if (q !== null && nullQ()) console.log(afterArg(q, tick())) });
console.log("ticks", ticks);
run("generic", () => { if (q !== null && nullQ()) console.log(rd(q)) });

// a list walk whose links end in null reads every live node
function sum(x: Node): number { return x.next === null ? x.n : x.n + sum(x.next) }
const list: Node = { n: 1, next: { n: 2, next: { n: 3, next: null } } };
console.log(sum(list), first(list), firstIf(list, 0), loop(list), rd(list), rd({ n: 4 }));
