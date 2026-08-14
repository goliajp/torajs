// 401-05 — §14.7.4: a for-head `var` declaration shares the
// function-level hoist domain whether or not it carries a type
// annotation. The typed form used to convert to a loop-scoped `let`,
// so a read after the loop answered "unknown identifier".

for (var x: any = 1; false; ) ;
console.log(x);
for (var y: number = 0; y < 3; y++) ;
console.log(y);
function f() {
  for (var i: any = 5; false; ) ;
  return i;
}
console.log(f());
// the untyped form keeps its existing hoist path
for (var u = 2; false; ) ;
console.log(u);
