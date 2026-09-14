// ES §23.1.3.{8-11,30}: a predicate's return goes through ToBoolean,
// so `return undefined` / `return Symbol('')` / `return {}` are all
// legal answers. The closure-param seeding strapped the positional
// "boolean" return annotation onto a body its sniff could not type,
// and `check_stmt_return` rejected the body before the call-site
// wedges could look (test262 find/return-*-predicate-* × 8,
// rotation 651). An unsniffable predicate body now seeds `any`.
var arr = ["Shoes", "Car", "Bike"];
console.log(arr.find(function (val) { return undefined; }));
console.log(arr.find(function (val) { return null; }));
console.log(arr.find(function (val) { return Symbol(""); }));
console.log(arr.find(function (val) { return {}; }));
console.log(arr.find(function (val) { return NaN; }), arr.find(function (val) { return ""; }));
console.log(arr.findLast(function (val) { return "string"; }));
console.log(arr.some(function (v) { return NaN; }), arr.every(function (v) { return "x"; }));
console.log(arr.filter(function (v) { return 0; }).length, arr.findIndex(function (v) { return v.length - 3; }));
console.log(arr.findLastIndex(function (v) { return v.length > 3 ? {} : null; }));
