// a binding initialized from a field or element read holds a closure
// cell only where it is in scope: a numeric binding of the same name
// elsewhere leaves a top-level function of that name passed as itself
type F = (x: number) => number;

function m(f: F): number { return f(1) }
function inc(x: number): number { return x + 1 }
function g(x: number): number { return x + 2 }
function h(x: number): number { return x + 3 }

function counter(o: { inc: number }) { const inc = o.inc; return inc * 10 }
function first(xs: number[]) { const g = xs[0]; return g + 1 }
function pickNum({ h }: { h: number }) { return h * 2 }
function blockScoped(o: { inc: number }) {
  {
    const inc = o.inc;
    console.log(inc);
  }
  return m(inc);
}

console.log(counter({ inc: 4 }), first([5]), pickNum({ h: 6 }));
console.log(m((x) => x * 7));
console.log(m(inc), m(g), m(h), blockScoped({ inc: 3 }));

// the cell-holding spellings, each sharing a top-level function's name
function viaObj({ inc }: { inc: F }) { return m(inc) }
function viaMember(p: { g: F }) { const g = p.g; return m(g) }
function viaIndex(fs: F[]) { const g = fs[0]; return m(g) }
function viaAnnotated(p: { h: F }) { const h: F = p.h; return m(h) }
function viaCapture({ g }: { g: F }) { const t = () => m(g); return t() }
function viaNestedDecl({ h }: { h: F }) {
  function t(): number { return m(h) }
  return t();
}
class C {
  k = 9;
  run({ inc }: { inc: F }): number { return m(inc) + this.k }
}

let k = 20;
console.log(viaObj({ inc: (x) => x + 50 }), viaObj({ inc }), viaObj({ inc: g }));
console.log(viaMember({ g: (x) => x + k }), viaMember({ g }));
console.log(viaIndex([(x) => x * 11, inc]), viaIndex([h]));
console.log(viaAnnotated({ h: (x) => x - k }), viaAnnotated({ h }));
console.log(viaCapture({ g: (x) => x * k }), viaCapture({ g: inc }));
console.log(viaNestedDecl({ h: (x) => x + 600 }), viaNestedDecl({ h: g }));
console.log(new C().run({ inc: (x) => x + k }), new C().run({ inc }));
console.log(m(inc), m(g), m(h));

// a fn-typed return of the top-level function is not a cell either
function pickInc(): F { return inc }
console.log(pickInc()(5));
