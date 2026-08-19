// Rotation 447 — `<regex-literal>[Symbol.replace](str, fn)`: the
// §22.2.6.11 protocol spelling. The computed symbol call reifies
// the RegExp proto's @@replace cell and flips the operands back
// into the Str home, so a this-reading replacer rides the same
// receiver-flag-aware boxed kernel as `.replace` — module code
// sees the strict-mode undefined receiver.
var thisVal: any = null;
var replacer = function (m: any, p1: any) {
  thisVal = this;
  return "<" + p1 + ">";
};
console.log(/x(y)?/[Symbol.replace]("axb", replacer));
console.log(thisVal === undefined);
console.log(/b/[Symbol.replace]("abc", function () { thisVal = this; return "-"; }));
console.log(thisVal === undefined);
