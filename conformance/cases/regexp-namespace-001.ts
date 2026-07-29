// The RegExp constructor as a VALUE. `new RegExp(...)` is owned by
// the builtin-new desugar and never reached the checker's ident
// table, so `RegExp` was the one builtin ctor missing from it —
// every `RegExp.prototype` / `RegExp.name` read answered "unknown
// identifier" and rejected the whole program. The lowerer had
// carried the ctor-namespace face (proto tag 7 + name + length) for
// the other thirteen builtins all along; RegExp only needed the
// checker to admit the ident.
console.log("typeof-ctor", typeof RegExp);
console.log("name", RegExp.name);
console.log("length", RegExp.length);
console.log("proto-nonnull", RegExp.prototype !== null);
console.log("typeof-proto", typeof RegExp.prototype);

// The prototype's own method surface and its property descriptors.
const p: any = RegExp.prototype;
console.log("exec", typeof p.exec);
console.log("test", typeof p.test);
console.log("hasOwn-exec", Object.prototype.hasOwnProperty.call(RegExp.prototype, "exec"));
console.log(
  "exec-desc",
  JSON.stringify(Object.getOwnPropertyDescriptor(RegExp.prototype, "exec")),
);

// Identity against a real instance — the same cell both faces read.
const r: any = new RegExp("a+", "g");
console.log("getproto-identity", Object.getPrototypeOf(r) === RegExp.prototype);
console.log("instanceof", r instanceof RegExp);

// The construct path itself is unchanged.
console.log("exec-result", JSON.stringify(r.exec("baaa")));
console.log("source", r.source, "flags", r.flags);
