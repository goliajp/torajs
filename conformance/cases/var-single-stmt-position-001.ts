// §14.6 / §14.7 / §14.13 hand their body a `Statement`, and §14.3.2
// VariableStatement is one — so a `var` needs no braces in an `if`
// branch, a loop body or a labelled body. It is also the only
// declaration form that may sit there: `let` / `const` / `class` are
// Declarations, which is what rotation 578's judge refuses.
//
// The binding hoists to the enclosing function or script whatever the
// body does, so the name is readable afterwards even when the branch
// never ran.

if (false) var a = 1;
console.log("if-not-taken", typeof a, a);
if (true) var b = 2;
console.log("if-taken", b);
if (false) var c = 3;
else var c2 = 4;
console.log("else-branch", typeof c, c2);

while (false) var d = 5;
console.log("while", typeof d);
do var e = 6;
while (false);
console.log("do-while", e);
for (var i = 0; i < 1; i++) var f = 7;
console.log("for", f, i);
for (var k in { z: 1 }) var g = 8;
console.log("for-in", g, k);
for (var v of [9]) var h = v;
console.log("for-of", h);

lbl: var m = 10;
console.log("labelled", m);

// No initializer: the binding exists and reads undefined.
if (false) var n;
console.log("no-init", typeof n);

// Inside a function the hoist stops at that function's body.
function host() {
  if (true) var inner = 11;
  return inner;
}
console.log("nested", host(), typeof inner);
