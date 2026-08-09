// rotation 346 — an Undefined-typed property key joins the keyed
// kernels' §7.1.19 ToPropertyKey dispatch: ToPropertyKey(undefined)
// is the fixed string key "undefined". The t262 dstr harness shape
// (`{}[thrower()]`, where the throwing callee types its result
// Undefined) needs the read AND assign lanes plus the struct
// receiver; a real undefined key round-trips through the expando.
var thrower = function () {
  throw new Error("boom");
};
var caught = "";
try {
  var tmp: any = [{}[thrower()]];
} catch (e: any) {
  caught = e.message;
}
console.log(caught);

var wcaught = "";
try {
  0, [{}[thrower()]] = [1];
} catch (e: any) {
  wcaught = e.message;
}
console.log(wcaught);

var o: any = {};
var u = undefined;
o[u] = 7;
console.log(o["undefined"], o[u]);
