// a narrowed array, string, instance or map that a call set to null:
// iterating, indexing, measuring or calling through it throws, and
// passing or printing it does not
class K { v = 1; m(): number { return this.v } }
let a: number[] | null = [1, 2];
let s: string | null = "abc";
let k: K | null = new K();
let mp: Map<string, number> | null = new Map();
function f(): boolean { a = null; s = null; k = null; mp = null; return true }
function reset(): void { a = [1, 2]; s = "abc"; k = new K(); mp = new Map() }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  reset();
}

run("for-of", () => { if (a !== null && f()) { for (const e of a) console.log(e) } });
run("length", () => { if (a !== null && f()) console.log(a.length) });
run("index", () => { if (a !== null && f()) console.log(a[0]) });
run("array spread", () => { if (a !== null && f()) console.log([...a].length) });
run("push", () => { if (a !== null && f()) a.push(3) });

function len(t: string): number { return t.length }
run("string", () => { if (s !== null && f()) console.log(s.length) });
run("string param", () => { if (s !== null && f()) console.log(len(s)) });
run("charCodeAt", () => { if (s !== null && f()) console.log(s.charCodeAt(0)) });
run("string print", () => { if (s !== null && f()) console.log(s, typeof s, s === null, s + "!") });

run("call", () => { if (k !== null && f()) console.log(k.m()) });
run("field", () => { if (k !== null && f()) console.log(k.v) });
run("instanceof", () => { if (k !== null && f()) console.log(k instanceof K) });
run("map", () => { if (mp !== null && f()) console.log(mp.get("x")) });
run("truthy", () => { if (k !== null && f()) console.log(k ? "t" : "f") });
