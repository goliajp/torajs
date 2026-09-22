// §26.1.1.1 / §13.3.5 — `new WeakRef()` with no target throws a
// TypeError at runtime (CanBeHeldWeakly(undefined) is false); extra
// arguments are evaluated, in order, and ignored.
const log: string[] = [];
function tag(s: string): number {
  log.push(s);
  return 0;
}
try {
  // @ts-ignore
  new WeakRef();
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
const o = { a: 1 };
// @ts-ignore
const w = new WeakRef(o, tag("x"), tag("y"));
console.log(w.deref() === o, log.join(","));
try {
  // @ts-ignore
  new WeakRef(1, tag("z"));
} catch (e) {
  console.log(e instanceof TypeError, log.join(","));
}
