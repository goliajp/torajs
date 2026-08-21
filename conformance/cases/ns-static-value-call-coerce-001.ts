// A builtin reached through a binding is still that builtin. The
// member table's signature names what each spec step COERCES its
// argument to -- §22.1.2.1 step 2 is ToUint16, §21.3.2.24 ToNumber --
// so it must not be enforced as a shape on the way in. The direct
// call form has read them that way since rotation 463; these arrive
// through a binding and land on the same kernel.

const fcc = String.fromCharCode;
console.log(fcc("65"), fcc(65), fcc(66, 67), fcc(true), fcc());

const fcp = String.fromCodePoint;
console.log(fcp("65"), fcp(0x1f600));

const max = Math.max;
console.log(max("3", 4), max(1, 2), max(5), max());

const floor = Math.floor;
console.log(floor("3.7"), floor(2.9), floor("abc"));

// The `.call` / `.apply` surface of the same values.
console.log(max.call(null, "3", 4), max.apply(null, [1, 9]));

// Reflection still reads the spec meta row, not the checker sig.
console.log(fcc.name, fcc.length, max.name, max.length);

// Non-numeric parameter shapes ride the same rule.
const keys = Object.keys;
console.log(keys({ a: 1, b: 2 }));
const parse = Date.parse;
console.log(typeof parse("2020-01-01T00:00:00Z"));

// The return type still describes the answer, so a typed slot binds.
const n: number = max("3", 4);
const s: string = fcc("65");
console.log(n, s, typeof n, typeof s);
