// §27.2.5.4 steps 3-4 — then / catch construct the derived promise
// through SpeciesConstructor(promise, %Promise%).
const log: string[] = [];
class MyP extends Promise<number> {}
const p = new MyP((r) => r(1));
const q = p.then((x) => x + 1);
log.push("then instanceof MyP " + (q instanceof MyP));
log.push("catch instanceof MyP " + (p.catch(() => 0) instanceof MyP));

// A per-instance constructor whose @@species is a Promise subclass.
let calls = 0;
let execLen = -1;
class Spec extends Promise<unknown> {
  constructor(ex: any) {
    super(ex);
    calls++;
    execLen = ex.length;
  }
}
const p1: any = new Promise(() => {});
p1.constructor = function () {};
p1.constructor[Symbol.species] = Spec;
const p2 = p1.then();
log.push(`custom species calls=${calls} execLen=${execLen} instance=${p2 instanceof Spec}`);

// constructor null / non-object throws synchronously.
const bad: any = Promise.resolve(1);
bad.constructor = null;
try {
  bad.then(() => 0);
  log.push("null ctor no throw");
} catch (e) {
  log.push("null ctor throws " + (e instanceof TypeError));
}

// The constructor GET happens exactly once.
let gets = 0;
const counted: any = Promise.resolve(5);
Object.defineProperty(counted, "constructor", {
  get() {
    gets++;
    return Promise;
  },
});
counted.then((v: number) => log.push("counted " + v + " gets=" + gets));

q.then((v) => log.push("q " + v));
async function flush() {
  for (let i = 0; i < 8; i++) await null;
  console.log(log.join("\n"));
}
flush();
