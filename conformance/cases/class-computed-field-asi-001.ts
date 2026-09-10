// §12.10 ASI supplies the `;` of a bare computed-key FieldDefinition at a line break:
// `[x] = 42; [10] = "meep"; ["not initialized"]` followed by the next member on its own line
// (test262 class/elements/multiple-definitions-computed-names).
var x = "b";
var C = class {
  foo = "foobar";
  m() { return 42 }
  [x] = 42; [10] = "meep"; ["not initialized"]
  m2() { return 39 }
  bar = "barbaz";
};
var c: any = new C();
console.log(c.m(), c.m2(), c.foo, c.bar, c.b, c[10], c["not initialized"], Object.prototype.hasOwnProperty.call(c, "not initialized"));
console.log(["foo", "b", "10", "not initialized", "bar"].map((k) => Object.prototype.hasOwnProperty.call(c, k)).join(","));
