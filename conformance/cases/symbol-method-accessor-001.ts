// A symbol-keyed method call is a Get followed by a Call, so an
// accessor-shaped entry has to run its getter first. The symbol probe
// answers an ACCESSOR sentinel and this lane tested it for "heap
// value", so `o[sym]()` on an accessor threw "value is not a function"
// about a property that resolves to one.

const sym = Symbol("m");

// accessor-shaped method entry
const o1: any = {};
Object.defineProperty(o1, sym, {
  get() { console.log("GET"); return () => 7; },
});
console.log(o1[sym]()); // GET / 7

// the receiver reaches the callee
const o2: any = { v: 5 };
Object.defineProperty(o2, sym, {
  get() { return function (this: any) { return this.v; }; },
});
console.log(o2[sym]()); // 5

// arguments pass through
const o3: any = {};
Object.defineProperty(o3, sym, {
  get() { return (a: number, b: number) => a + b; },
});
console.log(o3[sym](2, 3)); // 5

// repeated calls stay correct (each getter answer is released once)
const o4: any = {};
Object.defineProperty(o4, sym, { get() { return () => 1; } });
console.log(o4[sym]() + o4[sym]() + o4[sym]()); // 3

// data-property entry is unchanged
const o5: any = { [sym]: () => 9 };
console.log(o5[sym]()); // 9
