// A top-level `await p.then(f)` on an instance of a user subclass of
// Promise: the inherited `then` builds the derived promise through the
// subclass (§27.2.5.4 SpeciesConstructor), and the await adopts it
// through its own `then` lookup, one job later than a %Promise% would
// take.
const log: string[] = [];

function add1(x: number): number {
  return x + 1;
}

class Plain extends Promise<number> {}

const q = Plain.resolve(2);
const d = q.then(add1);
log.push("sub " + (d instanceof Plain));
log.push("b" + (await d));
Promise.resolve().then(() => log.push("job"));
log.push("c" + (await Plain.resolve(5).then(add1)));
const r = new Plain((res: (v: number) => void) => res(7));
log.push("d" + (await r.then(add1)));
console.log(log.join(","));
