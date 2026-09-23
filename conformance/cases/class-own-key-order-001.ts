// §10.2.5 ClassDefinitionEvaluation runs SetFunctionLength →
// SetFunctionName → MakeConstructor, so a class object's own keys are
// `length, name, prototype` — statics follow in declaration order.
// tr answered `name, prototype, length`: the desugar built the class
// object as an object literal and the dynobj keeps insertion order.
class C { static m() {} }
console.log(Object.getOwnPropertyNames(C).join(","));

class D { constructor(a: any, b: any) {} }
console.log(Object.getOwnPropertyNames(D).join(","), D.length, D.name);

const E = class {};
console.log(Object.getOwnPropertyNames(E).join(","));

class G extends C { static n() {} }
console.log(Object.getOwnPropertyNames(G).join(","));

class H { static ["s"]() {} static t = 1; }
console.log(Object.getOwnPropertyNames(H).join(","));

// A plain function's own keys are the same three, in the same order
// (§20.2.4) — that path was already correct and must stay so.
function f(a: any) {}
console.log(Object.getOwnPropertyNames(f).join(","));

// Static `name` / `length` shadows still land in spec slot order.
class K { static name = "kay"; static length = 9; }
console.log(Object.getOwnPropertyNames(K).join(","), K.name, K.length);
