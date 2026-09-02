// 562-04 — a class instance's prototype rows come from the REIFIED
// prototype (the object `getOwnPropertyNames` answers from), not the
// compile-time method table. The table names a row by the symbol the
// desugar minted, and a computed member's symbol is a `__ccm_<n>__`
// sentinel — a name no property has. Shape follows bun's
// `forEachProperty`: up to five [[Prototype]] hops, `constructor`
// always hidden, the fast / slow walk decided per hop, and a nearer
// prototype's entry overriding a farther one.
const k = "k1";
// `m` is declared BEFORE the computed pair: a computed member
// reifies after the plain ones rather than at its declaration
// position (562-07), so the two orders only agree this way round.
class Y {
  w = 2;
  m() {}
  get [k]() { return 9; }
  set [k](v: number) {}
}
console.log(new Y());
console.log(JSON.stringify(Object.getOwnPropertyNames(Y.prototype)));

class Tagged { v = 1; get [Symbol.toStringTag]() { return "XX"; } }
console.log(new Tagged());

// A prototype whose only own entries are `constructor` and an
// assigned (enumerable) tag: the fast walk hits nothing, so bun
// restarts on the slow walk and the tag prints as a row.
class OnlyTag { v = 2; }
(OnlyTag.prototype as any)[Symbol.toStringTag] = "OT";
console.log(new OnlyTag());
// One more visible row and the fast walk hits — the tag stays hidden.
class TagPlus { v = 3; }
(TagPlus.prototype as any)[Symbol.toStringTag] = "TP";
(TagPlus.prototype as any).extra = 7;
console.log(new TagPlus());
class TagMethod { v = 4; m2() {} }
(TagMethod.prototype as any)[Symbol.toStringTag] = "TM";
console.log(new TagMethod());

class Base { a = 1; m1() {} m2() {} get g() { return 1; } }
console.log(new Base());
class Sub extends Base { b = 2; m3() {} m1() {} }
console.log(new Sub());
console.log(JSON.stringify(Object.getOwnPropertyNames(Sub.prototype)));

class Plain { p = 1; }
console.log(new Plain());
class OnlyMethod { om() {} }
console.log(new OnlyMethod());
