// A binding written `let x;` has no declared type: §14.3.1 initializes
// it to undefined, and tsc types it as an evolving `any` that accepts
// every later shape. `desugar_uninit_let` promotes the first
// assignment into the init slot, which makes this spelling look like
// `let x = <that value>` — where tsc DOES check the second assignment
// against the first (TS2741). `Ast::spliced_uninit_inits` keeps the
// two apart so only the bare one widens.
var o;
o = { join: function() { return "ohai"; } };
o = {};
console.log("bare:", typeof o, Object.keys(o).length);

// Same keys is still one shape, so this one keeps its typed slot —
// the widen is for shapes a declared type could not reconcile, not
// for every reassignment.
var p;
p = { a: 1 };
p = { a: 2 };
console.log("same-keys:", p.a);

// A scalar family is unaffected: the judgment is the syntactic
// family first, and both of these are Num.
let i;
i = 0;
i = 1;
console.log("num:", i + 1);

// The parser's multi-name line is a flat statement sequence, not a
// scope, so each declarator reaches its own follow-up assignment.
var q, seen;
seen = 0;
q = { x: 1 };
q = { y: 2 };
seen = 1;
console.log("multi:", Object.keys(q).join(","), seen);
