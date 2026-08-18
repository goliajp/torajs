// A nested-block `var f = function () { …this… }` promotes through the
// hoisted-var profile (rotation 437): var_hoist splits it into a
// fn-scope `let f: any = Uninit` prelude plus one in-place assignment,
// and the fnexpr-this zero-face promote resolves the init through that
// one admitted write. Module goal — plain calls answer `undefined`.

// try-scoped declaration, call at top level
try {
  var f = function () { return this === undefined; };
} catch (e) {}
console.log(f());

// bare-block spelling, call inside the block
{
  var g = function () { return typeof this; };
  console.log(g());
}

// declaration and call both inside the try
try {
  var h = function () { this; return 7; };
  console.log(h());
} catch (e) {}
