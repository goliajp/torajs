// An un-annotated alias of a function is handed to a fn-typed
// parameter and also escapes elsewhere (an array element, an object
// property, an identity comparison), so the alias holds the
// function's closure cell and the parameter has to take one.

function dbl(x: number): number {
  return x * 2;
}
function app(f: (x: number) => number): number {
  return f(2);
}

const r = dbl;
const xs = [r];
console.log(app(r), xs.length, r === dbl);

function main(): void {
  const m = dbl;
  const o = { f: m };
  console.log(app(m), o.f(3));
}
main();
