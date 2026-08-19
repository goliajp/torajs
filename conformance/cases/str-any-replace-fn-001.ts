// §22.1.3.19 step 5 through the ANY method dispatch — a Closure-cell
// replaceValue on a wrapper/any receiver invokes per match (the
// static lane's runtime twin); non-callables keep the ToString leg.
var obj: any = new String("abcab");
console.log(obj.replaceAll("ab", function () { return "X"; }));
var sv: any = new String("ab");
var seen: any = [];
console.log(obj.replaceAll(sv, function (m: any, p: any, w: any) {
  seen.push([m, p, w]);
  return "<" + m + p + ">";
}));
console.log(seen.length, seen[0][0], seen[0][1], seen[1][1], seen[0][2]);
console.log(obj.replace("ab", function () { return "Y"; }));
console.log(obj.replace("ab", 42));
