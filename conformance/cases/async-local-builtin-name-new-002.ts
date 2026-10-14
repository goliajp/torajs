// An async function that never awaits keeps its body in a try block;
// a local there under a builtin's name still shadows the builtin for
// `new`, as does a catch parameter.

async function fnExpr() {
  function TypeError() {}
  const g = function () {
    return new (TypeError as any)();
  };
  console.log(g().constructor === TypeError);
}

async function direct() {
  function SyntaxError() {}
  const s = new (SyntaxError as any)();
  console.log(s.constructor === SyntaxError, typeof s);
}

async function constBinding() {
  const Error = function () {};
  const e = new (Error as any)();
  console.log(e.constructor === Error, typeof e);
}

function catchParam() {
  try {
    throw function () {};
  } catch (TypeError) {
    const x = new (TypeError as any)();
    console.log(x.constructor === TypeError, typeof x);
  }
}

async function builtinOutside() {
  {
    const RangeError = function () {};
    console.log(typeof new (RangeError as any)());
  }
  const r = new RangeError("r");
  console.log(r instanceof Error, r.message);
}

fnExpr();
direct();
constBinding();
catchParam();
builtinOutside();
