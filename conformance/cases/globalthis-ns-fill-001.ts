// RFC 20260807-global-object G2 fill extension — the JSON / Reflect
// namespace singletons and the interned eval cell join the
// globalThis fill list, so the dynamic lane answers the same
// identities the bare ident reads do.
var g: any = globalThis;
console.log(g.JSON === JSON);
console.log(g.Reflect === Reflect);
console.log(g.eval === eval);
console.log(g.Math === Math);
console.log(g.JSON.parse("[5]")[0]);
