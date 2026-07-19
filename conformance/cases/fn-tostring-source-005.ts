// RFC 20260719-fn-tostring-source B6 — namespace-static builtin
// methods (Math.max / console.log / JSON.stringify / Object.keys)
// answer the JSC named native form through every string face:
// .toString() / String() / template substitution / + concat.
console.log(Math.max.toString());
console.log(String(Math.max));
console.log(`${Math.min}`);
console.log("v=" + Math.max);
console.log(console.log.toString());
console.log(JSON.stringify.toString());
console.log(Object.keys.toString());
console.log(Math.floor.toString() === String(Math.floor));
