// A named declaration used as a value becomes a closure cell, and an
// `any`-element array slot holds one — `const z: any[] = [p]` already
// stores it and answers `z[0] === p`. Only the STORE-SITE spelling
// fell through: the census that decides which array bindings wrap a
// named-fn store asked for a fn-TYPED array annotation, so `any[]`
// was not in it and `z.push(p)` reached the lowering carrying a raw
// FnSig, where it panicked.
function p(a: any): string {
  return "p" + a;
}

// The spellings that already worked, kept so a regression that stops
// wrapping shows up as a diff rather than as this file agreeing by
// accident.
const lit: any[] = [p];
console.log(lit[0](1), lit[0] === p);

const cast: any[] = [];
cast.push(p as any);
console.log(cast[0](1));

const hop: any = p;
const viaHop: any[] = [];
viaHop.push(hop);
console.log(viaHop[0](1));

// The store site itself.
const z: any[] = [];
z.push(p);
console.log(z[0](1));

const read: any[] = [];
read.push(p);
const f = read[0];
console.log(f(1));

// `Array<any>` spells the same element type.
const g: Array<any> = [];
g.push(p);
console.log(g[0](2));

// A non-function value through the same slot is untouched.
const mixed: any[] = [];
mixed.push(5);
mixed.push(p);
console.log(mixed[0], mixed[1](3));
