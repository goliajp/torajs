// RFC 20260713-mono-check-specializations — implicit-generic fns
// calling other implicit-generic fns from inside their bodies.
// Pre-fix: same-name fresh typevars (__T1) blind-reused the caller's
// subst (I64 arg consumed as Str → SIGSEGV); distinct-arity callees
// had no retarget at all ("unknown function").

// minimal SIGSEGV shape: callee instantiated at I64 while caller runs at Str
function logIt(n) {
  console.log(n);
}
function outer(ch) {
  logIt(1);
}
outer("i");

// distinct arity — callee typevar list differs from caller's
function pick(a, b) {
  return b;
}
function viaPick(ch) {
  return pick(ch, 1);
}
console.log(viaPick("i"));

// mixed instantiations of one callee from one specialized body
function tag(n) {
  return "x" + n;
}
function both(ch) {
  return tag(1) + tag(ch);
}
console.log(both("i"));

// arithmetic in callee — silent-wrong shape pre-fix
function inc(n) {
  return n + 1;
}
function callInc(ch) {
  return inc(2);
}
console.log(callInc("i"));

// nested fn + charCodeAt argument — the test262 charInfo/hexString shape
function charInfo(ch) {
  function hexString(n) {
    let s = n.toString(16).toUpperCase();
    return "0000".slice(s.length) + s;
  }
  if (ch.length === 1) {
    return "U+" + hexString(ch.charCodeAt(0));
  }
  return "?";
}
console.log(charInfo("i"));
console.log(charInfo("Q"));

// generic-to-generic recursion through a helper
function double(n) {
  return n * 2;
}
function chain(x) {
  return double(double(x));
}
console.log(chain(3));
console.log(chain(10));

// caller instantiated at two types, callee pinned at one
function shout(s) {
  return s + "!";
}
function greet(v) {
  return shout("hi") + "/" + v;
}
console.log(greet("a"));
console.log(greet(7));
