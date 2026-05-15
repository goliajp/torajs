// T-27.c — built-in `f.length` (param count) and `f.name` (declared
// name) on Function values. Per ECMAScript §10.2 these are
// non-configurable own properties of Function objects. Both are
// compile-time constants knowable from the FnDecl's static signature
// (parser-recorded params + the ident text), so ssa_lower folds them
// without runtime dispatch — no closure value or fn ptr operand
// needed; the AST FnDecl walk in `Expr::Member` resolves both.
//
// `f.bind(...)`, `f.call(...)`, `f.apply(...)` need runtime support
// (new closure with bound thisArg / unpacked args) and are tracked
// as L3b T-27.c-rest.
//
// Out of scope here: `Function.prototype.length === 0` (needs
// Function global + prototype chain — L3b T-27.b followup).

function add(a, b) { return a + b; }
console.log(add.length);   // 2
console.log(add.name);     // "add"
console.log(typeof add.length);  // number
console.log(typeof add.name);    // string

function noargs() { return 0; }
console.log(noargs.length);   // 0
console.log(noargs.name);     // "noargs"

function variadic(a, b, c, d, e) { return [a, b, c, d, e]; }
console.log(variadic.length);  // 5
console.log(variadic.name);    // "variadic"
