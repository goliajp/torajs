// a binding destructured out of a fn-typed field holds the field's
// value shape; forwarding it into a fn-typed parameter must not read
// it as a bare function
type F = (x: number) => number;

function dbl(x: number): number { return x * 2 }
function m(f: F): number { return f(1) }

function viaObj({ g }: { g: F }) { return m(g) }
function viaRenamed({ g: h }: { g: F }) { return m(h) }
function viaArr([g]: F[]) { return m(g) }
function viaNested({ a: { g } }: { a: { g: F } }) { return m(g) }
function viaDefault({ g = dbl }: { g?: F }) { return m(g) }
function viaLet(p: { g: F }) { const { g } = p; return m(g) }
function viaMember(p: { g: F }) { const g = p.g; return m(g) }
function viaIndex(fs: F[]) { const g = fs[1]; return m(g) }

console.log(viaObj({ g: dbl }));
console.log(viaObj({ g: (x) => x + 10 }));
let k = 7;
console.log(viaObj({ g: (x) => x + k }));
console.log(viaObj({ g: function (x) { return x - 5 } }));
console.log(viaRenamed({ g: dbl }));
console.log(viaArr([dbl]));
console.log(viaArr([(x) => x * k]));
console.log(viaNested({ a: { g: dbl } }));
console.log(viaDefault({}));
console.log(viaDefault({ g: (x) => x + 100 }));
console.log(viaLet({ g: dbl }));
console.log(viaMember({ g: (x) => x + k }));
console.log(viaIndex([dbl, (x) => x * 3]));

class C {
  base = 40;
  run({ g }: { g: F }): number { return m(g) + this.base }
}
console.log(new C().run({ g: dbl }));
console.log(new C().run({ g: (x) => x + k }));

// a top-level function sharing the destructured name still reaches
// the same parameter as itself
function g(x: number): number { return x + 1000 }
console.log(m(g));
console.log(viaObj({ g }));

// a choice between a top-level function and a field's value, either
// order, is called as whichever arm ran
function pick(c: boolean, p: { g: F }) { const h = c ? dbl : p.g; return m(h) + h(2) }
function pickRev(c: boolean, p: { g: F }) { const h = c ? p.g : dbl; return h(3) }
console.log(pick(true, { g: (x) => x + 5 }), pick(false, { g: (x) => x + 5 }));
console.log(pickRev(true, { g: (x) => x + k }), pickRev(false, { g: (x) => x + k }));
