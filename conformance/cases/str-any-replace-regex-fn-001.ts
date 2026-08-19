// §22.1.3.19 step 5 over a RegExp pattern through the ANY method
// dispatch — a Closure-cell replaceValue on a wrapper/any receiver
// invokes per match with «matched, p1..pn, position, string» shaped
// by the pattern's own capture count; a non-participating group
// reads undefined, and non-callables keep the ToString leg.
var obj: any = new String("xzabxz");
var re: any = /x(y)?(z)/g;
var seen: any = [];
console.log(obj.replace(re, function (m: any, p1: any, p2: any, off: any, w: any) {
  seen.push([m, p1, p2, off, w]);
  return "<" + p2 + ">";
}));
console.log(seen.length, seen[0][0], seen[0][1], seen[0][2], seen[0][3], seen[0][4]);
console.log(seen[1][3]);
var re2: any = /ab/;
console.log(obj.replace(re2, function (m: any) { return m.toUpperCase(); }));
console.log(obj.replaceAll(/xz/g, function () { return "-"; }));
console.log(obj.replace(re, 7));
try {
  obj.replaceAll(re2, function () { return "!"; });
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}
