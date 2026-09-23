// §15.4.1 — SuperProperty is legal anywhere inside a MethodDefinition,
// its FormalParameters included: a parameter's default initializer is
// evaluated with the method's [[HomeObject]] and `this` in scope. Both
// object-method parsers raised the position flag only around the BODY,
// so `{ m(x = super.toString) {} }` was refused at parse.
const sup: any = { method() { return "sup"; }, tag: "S" };

const o1: any = { __proto__: sup, m(x: any = super.method()) { return x; } };
console.log(o1.m(), o1.m("given"));

const o2: any = { __proto__: sup, m(x: any = super.tag) { return x; } };
console.log(o2.m());

// The default sees `this` and the preceding parameters too, and the
// two mix in one signature.
const o3: any = { __proto__: sup, v: 7, m(a: any, b: any = a + 1, c: any = super.tag, d: any = this.v) {
  return [a, b, c, d].join(",");
} };
console.log(o3.m(1), o3.m(1, 2, 3, 4));

// §13.3.7 GetSuperBase re-reads the prototype per access, so a later
// setPrototypeOf changes what a default sees.
const other: any = { method() { return "other"; }, tag: "O" };
const o4: any = { __proto__: sup, m(x: any = super.tag) { return x; } };
console.log(o4.m());
Object.setPrototypeOf(o4, other);
console.log(o4.m());

// The body form keeps working next to the parameter form.
const o5: any = { __proto__: sup, m(x: any = super.tag) { return x + super.method(); } };
console.log(o5.m());
