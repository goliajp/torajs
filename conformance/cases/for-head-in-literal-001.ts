// §13.2.4 / §13.2.5 — the for-head [In] restriction does not reach
// inside an array or object literal: every element, property value,
// and computed property name is AssignmentExpression[+In]
// (regression: accessor-name-computed-in — `for (o = { get ['x' in
// e]() {} };;)` is legal and the [In] gate refused it at parse).
var empty: any = Object.create(null);
var obj: any, value: any;
for (obj = { get [("x" in empty) as any]() { return 8; } } as any; ; ) {
  value = obj.false;
  break;
}
console.log(value);

for (const q of [("y" in empty)]) {
  console.log(q);
}

for (obj = { k: "z" in empty } as any; ; ) {
  console.log(obj.k);
  break;
}
