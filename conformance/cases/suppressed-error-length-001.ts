// RFC 20260809 B6 residual — §20.5.8 SuppressedError's ctor length
// is 3 even though `new SuppressedError()` is legal (the injected
// class carries all-optional params, so the natural length would be
// 0); a defineProperty override in the injected prefix pins it.
console.log((SuppressedError as any).length);
console.log((SuppressedError as any).name);
const se: any = new (SuppressedError as any)(1, 2);
console.log(se.error);
console.log(se.suppressed);
console.log(se.message === "");
const s0: any = new (SuppressedError as any)();
console.log(s0.error);
console.log("end");
