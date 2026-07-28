let callCount = 0;
class C {
  method(a, b = 39) {
    callCount = callCount + 1;
    return a + b;
  }
}
const ref = C.prototype.method;
console.log(ref(42, undefined), callCount);
