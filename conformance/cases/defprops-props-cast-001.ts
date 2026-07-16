// an `as any` cast on an inline struct-literal props container is an
// SSA pass-through — the runtime walk must still fire (pre-fix this
// shape eval-dropped: no defines, no non-object-desc TypeError)
const o: any = {};
Object.defineProperties(o, { a: { value: 1, enumerable: true } } as any);
console.log(o.a);

const q: any = {};
let caught = "";
try {
  Object.defineProperties(q, { bad: 5 } as any);
} catch (e: any) {
  caught = e.name;
}
console.log(caught);
console.log(Object.keys(q).length);
