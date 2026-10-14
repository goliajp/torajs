// §16.2.3 — every declarator of an exported declaration is one of the
// module's ExportedNames, so `export var a, a;` exports `a` twice and
// is a parse-phase SyntaxError (module-only, so not probed here). The
// distinct-name lists below, destructuring included, stay legal.
export var a1 = 1,
  b1 = 2;
export const [c1, d1] = [3, 4];
export let { e1, f1 } = { e1: 5, f1: 6 };
console.log(a1 + b1 + c1 + d1 + e1 + f1);
