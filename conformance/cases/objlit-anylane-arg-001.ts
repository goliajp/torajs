// RFC 20260717-objlit-anylane-recv knife 2g — an inline ObjectLit
// argument at an any-lane call site (bare any-call / any-method-call
// / optcall) rides the dynobj lane instead of packing a struct cell
// the Any-gated kernels misdecode (entries answered [null]).
const f: any = Object.keys;
console.log(f({ a: 1, b: 2 }).length);
const g: any = Object.entries;
console.log(JSON.stringify(g({ x: 5 })));
const h: any = Object.values;
console.log(JSON.stringify(h({ p: 3, q: 4 })));
const holder: any = { keys: Object.keys };
console.log(holder.keys({ a: 1, b: 2, c: 3 }).length);
console.log(JSON.stringify(g?.({ y: 9 })));
console.log(JSON.stringify(h({ u: undefined })));
console.log(JSON.stringify(g({ outer: { inner: 1 } })));
