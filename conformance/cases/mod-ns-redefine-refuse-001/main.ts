// §10.4.6.6 — [[DefineOwnProperty]] on an EXISTING namespace key. The
// entry looks perfectly ordinary — { writable: true, enumerable: true,
// configurable: false } — and an ordinary receiver carrying exactly
// those attributes would accept a new value, because a
// non-configurable-but-writable data property is allowed to change.
// Through a namespace it is not: the descriptor may only restate what
// is already there.
//
// Steps 4-8 each get a line. The last one is the only accepting shape:
// a descriptor that asks for nothing different at all.
import * as ns from "./lib";
const n: any = ns;

console.log(Reflect.defineProperty(n, "a", { value: 9 }));
console.log(Reflect.defineProperty(n, "a", { configurable: true }));
console.log(Reflect.defineProperty(n, "a", { enumerable: false }));
console.log(Reflect.defineProperty(n, "a", { writable: false }));
console.log(Reflect.defineProperty(n, "a", { value: 1 }));
console.log(Reflect.defineProperty(n, "a", { enumerable: true }));

try { Object.defineProperty(n, "a", { value: 9 }); console.log("no throw"); }
catch (e: any) { console.log("throw", e.constructor.name); }

console.log(n.a, JSON.stringify(Object.getOwnPropertyDescriptor(n, "a")));
