// `Object.freeze(xs)` makes an array's own `length` non-writable
// (§7.3.14 on every own property), and the length lock is the bit
// every reader of that attribute consults — the descriptor, the
// define validation, the writers. Before this a frozen array's
// `length` descriptor still said writable: true.
const xs: any = [1, 2]; Object.freeze(xs);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(xs, "length")));
const ys: any = [1]; Object.seal(ys); console.log(JSON.stringify(Object.getOwnPropertyDescriptor(ys, "length")));
const zs: any = [1]; Object.defineProperty(zs, "length", { writable: false }); console.log(JSON.stringify(Object.getOwnPropertyDescriptor(zs, "length")), Object.isFrozen(zs), Object.isSealed(zs));
try { Reflect.defineProperty(xs, "length", { value: 2 }); console.log("same ok", Reflect.defineProperty(xs, "length", { value: 2 })); } catch (e) { console.log("threw", (e as Error).message); }
console.log(Reflect.defineProperty(xs, "length", { value: 1 }), xs.length);
