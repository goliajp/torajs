// nullish .length reads throw a catchable TypeError (RFC 20260707
// residual: env-miss bindings + string-index OOB slots hold the
// undefined sentinel; the inline length load must guard, §13.3.2.1).
const v = process.env.__TORAJS_MISSING__!;
let caught1 = false;
try {
  console.log(v.length);
} catch (e) {
  caught1 = true;
}
console.log(caught1);
console.log(typeof v);
console.log(v === undefined);
console.log(v == null);
console.log(v === "undefined");
// direct member-chain length
let caught2 = false;
try {
  console.log(process.env.__TORAJS_MISSING2__!.length);
} catch (e) {
  caught2 = true;
}
console.log(caught2);
// hit lane — PATH is always set in the test environment
const p = process.env.PATH!;
console.log(typeof p);
console.log(p !== undefined);
console.log(p.length > 0);
// string-index OOB .length guards too
const s = "ab";
let caught3 = false;
try {
  console.log(s[5].length);
} catch (e) {
  caught3 = true;
}
console.log(caught3);
// alias of an index read
const d = s[9];
let caught4 = false;
try {
  console.log(d.length);
} catch (e) {
  caught4 = true;
}
console.log(caught4);
// in-range index length stays working (guarded but passes)
console.log(s[1].length);
const c = s[0];
console.log(c.length);
