// chunk 721 — console releases owned Any temps (single + multi arg
// + optcall discard); borrowed operands stay alive after the print
function mk(i: number): any {
  return "fresh-" + i;
}
console.log(mk(1));
console.log("pfx", mk(2));
// borrowed ident survives the print and stays usable
const keep: any = "keep-alive-value";
console.log(keep);
console.log("pfx", keep);
console.log(keep, keep);
console.log(keep.length, keep);
// owned member read in multi-arg position (chunk 717 family)
const re: any = /abc/g;
console.log("src:", re.source, "flags:", re.flags);
console.log(re.source, re.source);
// optcall result printed + discarded
const f: any = mk;
console.log(f?.(3));
f?.(4);
console.log("after", keep);
// binop any temp in both positions
const a: any = 20;
const b: any = 22;
console.log(a + b, "mixed", a + b);
