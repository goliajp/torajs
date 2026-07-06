// RC-4 arguments-object 10.6-6-4 — indirect calls (IIFE / closure
// local / fn-typed param / chained) with fewer args than declared
// params must pad the missing trailing Any slots with undefined
// (T-28, ES §10.2.1.4). Before the pad the CallIndirect argv was
// shorter than the signature and the callee read garbage registers —
// materializing the params into an any[] SIGSEGV'd.

// IIFE, 0 of 3 args
(function (a, b, c) {
  const arr: any[] = [a, b, c];
  console.log(arr.length);
  console.log(a === undefined);
})();

// closure-typed local, 0 of 3 args
const g = function (a, b, c) {
  const arr: any[] = [a, b, c];
  console.log(arr.length);
  console.log(typeof b);
};
g();

// partial args: 1 of 3
const h = function (a, b, c) {
  console.log(a);
  console.log(b === undefined);
  console.log(c === undefined);
};
h(7);

// fn-typed param called with 0 args
function call0(f: (a: any, b: any, c: any) => void) {
  f();
}
call0(function (a, b, c) {
  console.log(c === undefined);
});

// chained call, 0 of 3 args
function mk() {
  return function (a, b, c) {
    const arr: any[] = [a, b, c];
    console.log(arr.length);
  };
}
mk()();
