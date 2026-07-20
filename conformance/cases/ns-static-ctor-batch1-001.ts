// ctor statics read as VALUES (RFC 20260720-ctor-static-reflection
// 刀 1) — Date.now/parse/UTC, String.fromCharCode/fromCodePoint,
// Object.hasOwn resolve interned dispatcher cells: name/length
// reflection, real dispatch through the variadic boxed lane, spec
// edge semantics (ToUint16 wrap, RangeError, nullish TypeError,
// absent-arg defaults) and interned identity.
const dn: any = Date.now;
console.log(typeof dn, dn.name, dn.length);
const t = dn();
console.log(typeof t, t > 1500000000000);
const dp: any = Date.parse;
console.log(dp.name, dp.length, dp("2020-01-02T03:04:05.006Z"));
const du: any = Date.UTC;
console.log(du.name, du.length, du(2020, 0, 2));
console.log(du());
const fcc: any = String.fromCharCode;
console.log(fcc.name, fcc.length, fcc(72, 105));
console.log(JSON.stringify(fcc()), fcc(65601));
const fcp: any = String.fromCodePoint;
console.log(fcp.name, fcp.length, fcp(128512).length);
try { fcp(1.5); } catch (e) { console.log("cp-frac", (e as Error).name); }
try { fcp(-1); } catch (e) { console.log("cp-neg", (e as Error).name); }
const ho: any = Object.hasOwn;
console.log(ho.name, ho.length, ho({a: 1}, "a"), ho({a: 1}, "b"));
try { ho(null, "a"); } catch (e) { console.log("ho-null", (e as Error).name); }
console.log(Date.now === Date.now, Date.parse === Date.parse);
