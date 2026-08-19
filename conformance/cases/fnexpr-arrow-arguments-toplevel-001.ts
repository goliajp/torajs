// §9.4.4 — an arrow has no `arguments` of its own; a read inside one
// resolves to the nearest enclosing NON-ARROW function's object. The
// alias pass covered fn-exprs nested in FnDecl bodies, but its
// top-level entry only walked FnDecl statements — a fn-expr stored by
// any other top-level statement (`Promise.resolve = fn`, `const f =
// fn`, `o.m = fn`) was never scanned, so its arrow-interior
// `arguments` read stayed on the lifted closure's own (empty) argc
// and answered undefined. Three previously-missed store shapes plus
// the FnDecl-interior control.
(Promise as any).resolve = function () {
  const get = () => arguments[0];
  return get();
};
const r: any = (Promise as any).resolve(7);
console.log("member-builtin", r);

const o: any = {};
o.m = function () {
  const get = () => arguments[0];
  return get();
};
console.log("member-obj", o.m(9));

const f: any = function () {
  const get = () => arguments.length;
  return get();
};
console.log("const-store", f(1, 2, 3));

function outer() {
  const h = function () {
    const get = () => arguments[0];
    return get();
  };
  return h(5);
}
console.log("fndecl-interior", outer());
