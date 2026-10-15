// `instanceof` against a local named after a builtin constructor asks
// that binding, not the intrinsic.

const SyntaxError = function () {};
const top = new (SyntaxError as any)();
console.log(top instanceof SyntaxError);

function fnValue() {
  const RangeError = function () {};
  const y = new (RangeError as any)();
  console.log(y instanceof RangeError, new Error("e") instanceof RangeError);
}

function fnDecl() {
  function TypeError() {}
  const t = new (TypeError as any)();
  console.log(t instanceof TypeError, {} instanceof TypeError);
}

function param(Error: any) {
  const e = new Error();
  console.log(e instanceof Error, 1 instanceof Error);
}

function objectValue() {
  const Map: any = { [Symbol.hasInstance]: (v: any) => v === 7 };
  console.log(7 instanceof Map, 8 instanceof Map);
}

async function inAsync() {
  const Error = function () {};
  await null;
  const e = new (Error as any)();
  console.log(e instanceof Error);
}

function builtinStillBuiltin() {
  {
    const TypeError = function () {};
    console.log(new (TypeError as any)() instanceof TypeError);
  }
  console.log(new TypeError("t") instanceof TypeError, new TypeError("t") instanceof Error);
}

fnValue();
fnDecl();
param(function () {});
objectValue();
inAsync();
builtinStillBuiltin();
