// RFC 20260708-closure-argv-face return arm — a fn-expr whose value
// escapes through a RETURN joins the argv face: the caller holds an
// opaque closure value, every call rides the boxed adapter, and the
// enclosing fn's inferred return sig publishes the rest-tail
// spelling.
// a1: returned directly, arguments VALUES touched
function f1() {
  return function () {
    return arguments[1];
  };
}
console.log("a1", f1()("x", "y"));
// a2: length-only twin (the argc tier served this before; must stay)
function f2() {
  return function () {
    return arguments.length;
  };
}
console.log("a2", f2()(10, 20, 30));
// a3: bound to a local, then returned
function f3() {
  const g = function () {
    return arguments[0];
  };
  return g;
}
console.log("a3", f3()(7));
// a4: spread of arguments inside the escaped body
function f4() {
  return function () {
    const xs = [...arguments];
    return xs.length;
  };
}
console.log("a4", f4()(1, 2, 3, 4));
// a5: annotated-any return keeps working
function f5(): any {
  return function () {
    return arguments[2];
  };
}
console.log("a5", f5()("p", "q", "r"));
console.log("done");
