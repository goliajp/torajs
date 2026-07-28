class C {
  method(a, b) { return [a, b] }
}
const ref = C.prototype.method;
console.log(ref(1, 2));
console.log(ref(1));
