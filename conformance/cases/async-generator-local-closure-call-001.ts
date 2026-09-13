// A machine class's factory literal initializes every lifted local with
// `undefined`; a closure field's `undefined` lowers with a placeholder
// signature, and when that placeholder differed from the declared slot
// (signature ids move with every signature the program minted before)
// the literal lost its declared layout: its `any` fields' `undefined`
// became the generic undefined CELL, so an async generator's request
// queue slot read back as a non-undefined object and the driver pushed
// onto it — "value is not a function on this any receiver" for any
// async generator whose body creates a closure, in exactly the programs
// whose signature count had shifted (a caller with two try regions).
async function* c0() { yield 0; yield 1; }
async function* cb() { const g = () => 7; yield 0; yield 1; }
async function* cc() { const g = () => 7; yield 0; yield g(); }
async function* cd() { let n = 1; const g = () => n; yield 0; n = 2; yield g(); }
async function* ce() { const sum = (...xs: number[]) => xs.length; yield sum(1, 2); yield sum(3, 4, 5); }
async function run(name: string, k: any) {
  const it: any = k();
  try { const r1: any = await it.next(); console.log(name, "s1", r1.value); } catch (e: any) { console.log(name, "s1 rej", String(e)); return; }
  try { const r2: any = await it.next(); console.log(name, "s2", r2.value); } catch (e: any) { console.log(name, "s2 rej", String(e)); }
}
async function main() { await run("c0", c0); await run("cb", cb); await run("cc", cc); await run("cd", cd); await run("ce", ce); }
main();
