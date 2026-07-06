// RC-4 F1c — reflection sees runtime-defined properties. Three
// coordinated pieces: (1) a defineProperty receiver's unannotated
// ObjectLit binding types as `any` (define converts the cell to a
// DynObj and the write-back only rebinds Any slots — a static struct
// type stranded the property on an orphan cell); (2) the any-route
// keys/gOPN kernel walks DynObj cells (was a loud non-struct
// TypeError); (3) keys filters enumerable-only per spec while
// getOwnPropertyNames includes all own keys.

let a = {};
Object.defineProperty(a, "x", { get: function() { return "gx"; }, configurable: true });
console.log(a.x);
console.log(Object.getOwnPropertyNames(a));
console.log(Object.keys(a));

let b = { v: 1 };
Object.defineProperty(b, "y", { value: 2, enumerable: true });
console.log(Object.getOwnPropertyNames(b));
console.log(Object.keys(b));
console.log(b.v, b.y);

let c = { v: 3 };
Object.defineProperty(c, "hidden", { value: 4, enumerable: false });
console.log(Object.getOwnPropertyNames(c));
console.log(Object.keys(c));
