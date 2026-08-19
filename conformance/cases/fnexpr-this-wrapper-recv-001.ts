// Rotation 447 — the wrapper-receiver replace family joins the
// cb-slot face: a `new String(...)` receiver checker-types Any, so
// the call rides the runtime any-dispatch whose replace kernels
// (literal glue AND the regex lane's boxed kernel) all read the
// receiver-first flag; a this-reading callback promotes instead of
// the unclaimed loud reject, and module code sees the strict-mode
// undefined receiver (§22.1.3.18 step 10).
var calls: any = [];
console.log(new String("xzab").replace("ab", function (m: any) {
  calls.push(this === undefined, m);
  return "-";
}));
console.log(calls[0], calls[1]);
console.log(new String("xzabxz").replace(/x(y)?(z)/g, function (m: any, p1: any, p2: any, off: any, w: any) {
  calls.push(this === undefined, p1 === undefined, p2, off, w);
  return "<" + p2 + ">";
}));
console.log(calls[2], calls[3], calls[4], calls[5], calls[6]);
var f = function (m: any) { calls.push(this === undefined); return m.toUpperCase(); };
console.log(new String("ab-ab").replaceAll("ab", f));
console.log(calls.length);
