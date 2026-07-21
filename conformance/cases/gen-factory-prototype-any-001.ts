// Generator factory `.prototype` through the any lane (G2) — the
// fncell mint installs the __Gen class proto, so the face answers
// the same object the instances inherit from.
function* sg() { yield 1; }
async function* ag() { yield 2; }
let g: any = sg;
console.log(typeof g.prototype);
let a: any = ag;
console.log(typeof a.prototype);
let it = sg();
console.log(Object.getPrototypeOf(it) === g.prototype);
console.log(typeof sg.prototype, typeof ag.prototype);
let r = it.next();
console.log(r.value, r.done);
