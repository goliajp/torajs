// §10.4.4.6 — unmapped arguments elements are plain deletable data
// properties: `delete arguments[i]` rides the materialized array's
// index-delete (hole shadow entry). Two gaps closed (10.5-7-b-4-s):
// the mutation walk now classifies the delete as a write (the
// snapshot substitution turned the target into a bare param name),
// and the rewrite recurses into Delete nodes.
function f(a: any, b: any) {
  var before = arguments[0] === 30 && arguments[1] === 12;
  delete arguments[1];
  var after = arguments[0] === 30 && typeof arguments[1] === "undefined";
  console.log(before, after, arguments.length);
}
f(30, 12);
