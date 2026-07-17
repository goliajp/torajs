// RFC 20260718-builtin-error-ctor-first-class 刀 2 — EvalError /
// URIError join the injected NativeError family (§20.5.5): first-class
// ctor value, §20.5.6.2 ctor chain, §20.5.6.3 prototype own name,
// instance faces. (tr itself never throws these two — no runtime
// throw-registry slot needed.)
console.log(typeof EvalError, typeof URIError);
console.log(Object.getPrototypeOf(EvalError) === Error);
console.log(Object.getPrototypeOf(URIError) === Error);
console.log((EvalError.prototype as any).name, (URIError.prototype as any).name);
const e = new EvalError("m");
console.log(e.name, e.message);
console.log(e instanceof EvalError, e instanceof Error);
console.log(EvalError.name, EvalError.length);
console.log(EvalError.prototype.constructor === EvalError);
const u = new URIError("");
console.log(u.name, u instanceof URIError);
