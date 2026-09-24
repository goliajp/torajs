// §10.4.4 — an arguments object's `length` is an ordinary data
// property: writing a non-length value stores it (no ArraySetLength
// RangeError, no truncation), and the descriptor keeps its attributes.
function t(...r: any[]) {
  const a: any = arguments;
  a.length = "unlikely";
  console.log(a.length, JSON.stringify(Object.getOwnPropertyDescriptor(a, "length")), a[0], Object.getOwnPropertyNames(a).includes("length"));
  a.length = 1;
  console.log(a.length, a[1], Object.keys(a).join());
  delete a.length;
  console.log(a.length, "length" in a);
  a.length = 7;
  console.log(a.length, JSON.stringify(Object.getOwnPropertyDescriptor(a, "length")), Object.keys(a).join());
}
t(10, 20);
function z() { const a: any = arguments; a.length = { v: 1 }; return a.length.v; }
console.log(z(), z(1, 2, 3));
