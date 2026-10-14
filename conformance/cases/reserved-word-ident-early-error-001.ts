// §13.1.1 — a ReservedWord is never an IdentifierReference, a
// BindingIdentifier or a LabelIdentifier. Four of them (`in`, `with`,
// `enum`, `debugger`) reach the parser spelled like names, and the
// object-literal shorthand re-reads a property NAME (which may be any
// reserved word) as a reference. Each probe is a parse-phase
// SyntaxError raised when the eval runs; the legal shapes beside them
// still evaluate.
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("var in = 1;")), caught(() => eval("var with = 1;")));
console.log(caught(() => eval("var enum = 1;")), caught(() => eval("var debugger = 1;")));
console.log(caught(() => eval("in = 1;")), caught(() => eval("(debugger);")));
console.log(caught(() => eval("var f = enum => 1;")), caught(() => eval("function g(debugger) {}")));
console.log(caught(() => eval("({ this });")), caught(() => eval("({ true });")));
console.log(caught(() => eval("({ false });")), caught(() => eval("({ null });")));
console.log(caught(() => eval("({ typeof });")));
console.log(caught(() => eval("var l\\u0065t = 1;")));
console.log(caught(() => eval("class C { static { ({ await }); } }")));

// The same words as property names, the `in` operator, and the
// debugger statement stay legal.
const o = { in: 1, with: 2, enum: 3, debugger: 4, this: 5, true: 6 };
console.log(o.in, o.with, o.enum, o.debugger, o.this, o.true);
console.log("in" in o, "nope" in o);
debugger;
const async = 7;
console.log({ async }.async);
