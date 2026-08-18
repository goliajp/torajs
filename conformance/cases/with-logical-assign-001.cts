// §13.15.2 logical assignment inside `with (o)` — the parser desugars
// `x ??= v` to `x ?? (x = v)` with a cloned left operand; the with
// desugar must keep the pair under ONE §9.1.1.2.1 HasBinding guard
// (single ResolveBinding), route the read AND the write through the
// object when it supplies the name, and preserve short-circuit.
var log: any[] = [];
var o: any = {
  get x() {
    log.push("get-x");
    return undefined;
  },
  set x(v: any) {
    log.push("set-x:" + v);
  },
  y: 0,
  b: "truthy",
};
var x = "lex-x";
var y = "lex-y";
var b = "lex-b";
var z: any = undefined;
with (o) {
  x ??= "fx";
  y ||= "fy";
  b &&= "fb";
  z ??= "fz";
}
console.log(log.join(","));
console.log(o.y + " " + o.b);
console.log(x + " " + y + " " + b + " " + z);
