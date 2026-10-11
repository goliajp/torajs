// a body binding a parameter default would reach is a different binding
// (ECMA-262 §10.2.11 steps 27-28): every reference that resolves to it
// follows it, inner scopes that rebind the name keep their own, and a
// body `var` that shadows a parameter starts as the parameter's value
// while a closure in the defaults keeps the parameter itself
function scopes(y = K) {
  const K = 2;
  {
    let K = 3;
  }
  let s = 0;
  for (let K = 0; K < 3; K++) s += K;
  switch (y) {
    case 1: {
      let K = 9;
      s += K;
    }
  }
  const c = class {
    m() {
      return K;
    }
  };
  try {
    throw 7;
  } catch (K) {
    return [y, K, s, new c().m()].join(",");
  }
}
function blockVar(y = K) {
  var K = 2;
  {
    var K = 4;
  }
  return [y, K].join(",");
}
function ctor(y = K) {
  function K(this: any) {
    this.v = 5;
  }
  return [y, new (K as any)().v].join(",");
}
function innerDefault(y = K) {
  let K = 2;
  const inner = (z = K) => z;
  return [y, inner()].join(",");
}
function closureThenWrite(y = () => K) {
  let K = 2;
  K = 3;
  return [y(), K].join(",");
}
function destrDefault({ a } = { a: K }) {
  let K = 2;
  return [a, K].join(",");
}
function destrInner({ a = K }: any) {
  let K = 2;
  return [a, K].join(",");
}
function priorParam(a: any, b = a) {
  var a;
  return [a, b].join(",");
}
function keptParam(a: any, g = () => a) {
  var a = 2;
  return [a, g()].join(",");
}
function writtenParam(a: any, g = () => a, s = (v: any) => {
  a = v;
}) {
  var a;
  s(8);
  return [a, g()].join(",");
}
function fnOverParam(a: any, g = () => a) {
  function a() {}
  return [typeof a, g()].join(",");
}
function fnOverRead(a: any, b = a) {
  function a() {}
  return typeof b;
}
function varOverOuter(y = K) {
  var K = 2;
  K = K + 1;
  return [y, K].join(",");
}
function nestedDefault(y = K) {
  let K = 2;
  function g(z = K) {
    let K = 3;
    return [z, K].join(",");
  }
  return [y, g()].join(";");
}

const K = 1;
console.log("scopes:", scopes());
console.log("block var:", blockVar());
console.log("constructor function:", ctor());
console.log("inner default:", innerDefault());
console.log("closure then write:", closureThenWrite());
console.log("destructuring default:", destrDefault());
console.log("destructuring inner default:", destrInner({}));
console.log("prior parameter:", priorParam(3));
console.log("kept parameter:", keptParam(1));
console.log("written parameter:", writtenParam(1));
console.log("function over parameter:", fnOverParam(1));
console.log("function over read:", fnOverRead(1));
console.log("var over outer:", varOverOuter());
console.log("nested default:", nestedDefault());
