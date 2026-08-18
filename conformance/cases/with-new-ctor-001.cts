// §9.1.1.2.1 + §13.3.5 — inside `with (o)`, the constructor name of a
// `new` expression resolves through the object environment record.
// The parser stores that name as a string on the New node (no Ident
// child), so the desugar needs a dedicated arm: object-supplied ctor
// shadows the lexical one, absent property falls through, and the
// argument subtrees keep their own guards in both arms.
class Base {
  tag = "lexical";
  suffix = "";
  constructor(s?: any) {
    if (s !== undefined) {
      this.suffix = s;
    }
  }
}
var o: any = {
  Base: class {
    tag = "object";
    suffix = "";
    constructor(s?: any) {
      if (s !== undefined) {
        this.suffix = s;
      }
    }
  },
};
var arg = "outer";
with (o) {
  var a: any = new Base();
  console.log(a.tag);
  var b: any = new Base(arg);
  console.log(b.tag + ":" + b.suffix);
}
var p: any = { arg: "supplied" };
with (p) {
  var c: any = new Base(arg);
  console.log(c.tag + ":" + c.suffix);
}
