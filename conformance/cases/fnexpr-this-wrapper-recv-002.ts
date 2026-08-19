// Rotation 447 — the variable-routed String-wrapper receiver: a
// binding whose every reaching value is a `new String(...)` mint
// joins the replace cb-slot face through the strwrapper census
// (both decl spellings — the kept annotated decl and the
// var-hoisted uninit-decl + single mint assignment). The t262
// replaceValue-call-* shape: wrapper receiver, wrapper searchValue
// (pattern rides the runtime any dispatch), this-reading routed
// replacer.
var t = (function () { return this; })();
var calls: any = [];
var replaceValue = function (...args: any[]) {
  calls.push([this, ...args]);
  return "z";
};
var searchValue: any = new String("ab c");
var obj = new String("ab c ab cdab cab c");
var result = obj.replaceAll(searchValue, replaceValue);
console.log(calls.length, result);
console.log(calls[0][0] === t, calls[0][1], calls[0][2], calls[0][3] === obj.toString());
console.log(calls[1][2], calls[2][2], calls[3][2]);

var empty = new String("");
var ecalls: any = [];
console.log(empty.replaceAll(new String(""), function (...args: any[]) {
  ecalls.push([this, ...args]);
  return "abc";
}));
console.log(ecalls.length, ecalls[0][0] === t, ecalls[0][1], ecalls[0][2]);
