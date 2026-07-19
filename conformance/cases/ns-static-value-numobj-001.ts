// RFC 20260719-ns-static-value-reify B3a — Number / Array / Object
// statics as VALUES: parse kernels, no-coercion predicates,
// same-value, plus the chunk-357 isArray stub desugar retirement
// (real semantics through the dispatcher instead of a false stub).
const pi = Number.parseInt;
console.log(pi("42", 10));
console.log(pi("0x1f", 16));
const pf = Number.parseFloat;
console.log(pf("3.5abc"));
const ii = Number.isInteger;
console.log(ii(5));
console.log(ii(5.5));
const nn = Number.isNaN;
console.log(nn(0 / 0));
console.log(nn(1));
const fi = Number.isFinite;
console.log(fi(1e308));
const si = Number.isSafeInteger;
console.log(si(9007199254740991));
console.log(si(9007199254740992));
const ia = Array.isArray;
console.log(ia([1, 2]));
console.log(ia("no"));
const oi = Object.is;
console.log(oi(0 / 0, 0 / 0));
console.log(oi(0, -0));
console.log(oi(1, 1));
console.log(pi.name, pi.length);
console.log(ia.name, ia.length);
console.log(oi.name, oi.length);
console.log(Number.parseInt === Number.parseInt);
const anyp: any = Number.isInteger;
console.log(anyp(7), typeof anyp);
