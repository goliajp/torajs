// Chunk 691 — W-N-c Object.getOwnPropertySymbols: tr has no
// symbol-keyed property surface (a symbol index assignment rejects
// loud at typecheck), so the own-symbol list is statically empty
// for every receiver; undefined / null still throw at runtime per
// §20.1.2.11 (ToObject guard, probe-verified — error print shape
// differs from bun so the throw lanes stay out of this fixture).
console.log(Object.getOwnPropertySymbols({}));
console.log(Object.getOwnPropertySymbols({ a: 1 }));
console.log(Object.getOwnPropertySymbols([1, 2]));
console.log(Object.getOwnPropertySymbols("hi"));
const anyo: any = { k: 1 };
console.log(Object.getOwnPropertySymbols(anyo));
const syms = Object.getOwnPropertySymbols({ a: 1 });
console.log(syms.length);
