// RFC 20260713-defprop-residual-cluster chunk B — descriptor /
// Properties argument validation (§20.1.2.3.1 step 2 ToObject +
// §6.2.6.5 ToPropertyDescriptor step 1 + IsCallable on get/set).
try {
  Object.defineProperties({}, null);
  console.log("A no-throw");
} catch (e) {
  console.log("A threw");
}
try {
  Object.defineProperties({}, undefined);
  console.log("B no-throw");
} catch (e) {
  console.log("B threw");
}
try {
  Object.defineProperties({}, { a: null });
  console.log("C no-throw");
} catch (e) {
  console.log("C threw");
}
try {
  Object.defineProperty({}, "k", 5);
  console.log("D no-throw");
} catch (e) {
  console.log("D threw");
}
var obj = {};
try {
  Object.defineProperties(obj, { p: { get: [] } });
  console.log("E no-throw");
} catch (e) {
  console.log("E threw");
}
try {
  Object.defineProperty(obj, "q", { get: false });
  console.log("F no-throw");
} catch (e) {
  console.log("F threw");
}
try {
  Object.defineProperty(obj, "r", { set: 5 });
  console.log("G no-throw");
} catch (e) {
  console.log("G threw");
}
// A valid accessor still works through the fast path.
Object.defineProperty(obj, "s", {
  get: function () {
    return 7;
  },
});
console.log(obj.s);
// A runtime-Any descriptor that is a real object still defines.
var desc: any = { value: 9, enumerable: true };
Object.defineProperty(obj, "t", desc);
console.log(obj.t);
