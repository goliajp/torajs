// Pair with `var-array-literal-001` — same silent-wrong fix for
// object literals. `var obj = {a: 1, b: 2}; obj.a` returned
// "undefined" because the var-hoist promoted obj to Type::Any and
// Member-on-Any dispatched through dynobj_get (no path that
// resolves typed-struct fields). Keeping the Struct slot under
// the same `init_keeps_type` exception fixes it.

var p = {x: 10, y: 20};
console.log(p.x);
console.log(p.y);

var named = {name: "alpha", count: 7};
console.log(named.name);
console.log(named.count);

// Inside a fn body — var still hoists, type still kept.
function inner(): number {
  var local = {a: 100, b: 200};
  return local.a + local.b;
}
console.log(inner());
