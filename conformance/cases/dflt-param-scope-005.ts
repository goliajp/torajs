// a nested function declaration whose only read of its scope is in a
// parameter default still captures that scope: the default is evaluated
// in the function's own scope (ECMA-262 §10.2.11) wherever it is called
function returned(k: number) {
  function inner(y: number = k) {
    return y;
  }
  return inner;
}
const f = returned(3);
console.log("returned:", f(), f(1));

function sideEffect(k: number) {
  let calls = 0;
  function inner(y: number = k + calls++) {
    return y;
  }
  const a = inner();
  const b = inner();
  const c = inner(100);
  return [a, b, c, calls].join(",");
}
console.log("side effect:", sideEffect(10));

function order(k: number) {
  const log: string[] = [];
  function inner(a: any = (log.push("a"), 1), b: number = (log.push("b"), k)) {
    return a + b;
  }
  const r = inner();
  return [r, log.join("")].join(" ");
}
console.log("order:", order(2));

function shadowed(k: number) {
  function inner(y: number = k) {
    return y;
  }
  const h = (y: number = k) => y;
  {
    const k = 99;
    return [inner(), h(), k].join(",");
  }
}
console.log("caller block shadows:", shadowed(5));
