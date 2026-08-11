// §19.2.2-6 — global function properties read as bare VALUES answer
// the interned ns-static cells (identity/name/length/detached call).
const pi = parseInt;
console.log(typeof pi, pi.name, pi.length, pi("42", 16), pi("31", 10));
const pf = parseFloat;
console.log(pf.name, pf.length, pf("2.5e2"));
const inan = isNaN;
console.log(inan.name, inan("abc"), inan("42"));
const ifin = isFinite;
console.log(ifin.name, ifin("42"), ifin(Infinity));
const dc = decodeURIComponent;
console.log(dc.name, dc.length, dc("%C3%A9"));
const eu = encodeURI;
console.log(eu.name, eu("a b/c"));
console.log(parseInt === Number.parseInt, parseFloat === Number.parseFloat);
console.log(isNaN === Number.isNaN, isFinite === Number.isFinite);
console.log(encodeURIComponent.name, decodeURI.length);
// identity through the dynamic lane
const g: any = globalThis;
console.log(g["parseInt"] === parseInt, g["decodeURI"] === decodeURI);
// alias call through the boxed dual entry
const pf2 = parseFloat;
console.log(pf2("1") + pf2("2") + pf2("3"));
