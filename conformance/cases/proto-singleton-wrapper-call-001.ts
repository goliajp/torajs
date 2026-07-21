// RFC 20260722 刀 5 — primitive-wrapper prototype singletons answer
// direct method calls with wrapper semantics: Number.prototype IS a
// Number object ([[NumberData]] = +0, §21.1.3), String.prototype a
// String one ("", §22.1.3), Boolean.prototype a Boolean one (false,
// §20.3.3) — so the whole method family re-dispatches on the spec
// initial value, not just toString/valueOf.
console.log(Number.prototype.toFixed(0));        // "0"
console.log(Number.prototype.toFixed(2));        // "0.00"
console.log(Number.prototype.toExponential(1));  // "0.0e+0"
console.log(Number.prototype.toPrecision(3));    // "0.00"
console.log(Number.prototype.toString());        // "0"
console.log(Number.prototype.valueOf());         // 0
console.log(Number.prototype.toLocaleString());  // "0"
console.log(String.prototype.charAt(0));         // ""
console.log(String.prototype.toUpperCase());     // ""
console.log(String.prototype.indexOf("a"));      // -1
console.log(String.prototype.toString());        // ""
console.log(String.prototype.valueOf());         // ""
console.log(Boolean.prototype.toString());       // "false"
console.log(Boolean.prototype.valueOf());        // false
// a mid outside the family keeps the honest TypeError
try { (Number.prototype as any).getDate(); } catch (e) { console.log("TypeError ok"); }
// a monkey-patch is an own entry on the prototype — still shadows
(Number.prototype as any).toFixed = function() { return "patched"; };
console.log((Number.prototype as any).toFixed(0));
