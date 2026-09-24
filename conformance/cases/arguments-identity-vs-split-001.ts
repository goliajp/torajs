// The arguments-object identity has a bit of its own (Any cells only).
// It used to share bit 1 with a str_split block, so a split result
// answered like an arguments object and an arguments object like an
// Array. §20.1.3.6 / §7.2.2 / §10.4.4.7.
function args(a: any, b: any): any { return arguments; }
const g = args(1, "x");
console.log(Object.prototype.toString.call(g), Array.isArray(g), Object.getPrototypeOf(g) === Object.prototype, g.length, g[1]);
console.log(Array.prototype.filter.call(g, (v: any, i: any, o: any) => Object.prototype.toString.call(o) === "[object Arguments]").length);
const parts: any = "a-b-c".split("-");
console.log(Object.prototype.toString.call(parts), Array.isArray(parts), Object.getPrototypeOf(parts) === Array.prototype, parts.length);
try { "use strict"; delete parts.length; console.log("deleted", parts.length); } catch (e: any) { console.log(e.constructor === TypeError); }
const s2: any = Array.prototype.slice.call(g);
console.log(Object.prototype.toString.call(s2), Array.isArray(s2), JSON.stringify(s2));
for (let i = 0; i < 3; i++) { args(i, i); console.log(JSON.stringify("p-q".split("-"))); }
