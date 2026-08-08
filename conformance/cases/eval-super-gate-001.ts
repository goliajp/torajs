// r334 blade 6 — eval × super context gate (§19.2.1.1 steps 4-6 + step 12).
// A direct eval at a class-member site may contain SuperProperty and
// resolves it; everywhere else — direct in global/function code,
// indirect anywhere — the eval throws SyntaxError at evaluation time
// and NOTHING in the source runs.

// 1. direct eval super inside class methods keeps resolving
class A {
  greet() {
    return "hi";
  }
}
class B extends A {
  m() {
    return eval("super.greet()");
  }
  r() {
    return eval("super.greet");
  }
}
const b = new B();
console.log(b.m());
console.log(typeof b.r());

// 2. global direct eval super -> runtime SyntaxError
var caught: any = null;
try {
  eval("super.property;");
} catch (e) {
  caught = e;
}
console.log(caught instanceof SyntaxError);

// 3. ordinary function body: index form with a side effect in the
// brackets -> SyntaxError before anything evaluates
var evaluated = false;
function f() {
  try {
    eval("super[evaluated = true];");
  } catch (_) {}
}
f();
console.log(evaluated);

// 4. arrow at the top level pierces to global code -> SyntaxError
var caught2: any = null;
var g = () => eval("super.property;");
try {
  g();
} catch (e) {
  caught2 = e;
}
console.log(caught2 instanceof SyntaxError);

// 5. indirect eval in a field initializer: throws at construction,
// the source's leading statement never runs
var executed = false;
class Base {}
class C extends Base {
  x = (0, eval)("executed = true; super.x;");
}
var caught3: any = null;
try {
  new C();
} catch (e) {
  caught3 = e;
}
console.log(caught3 instanceof SyntaxError, executed);
