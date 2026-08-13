// A symbol-keyed write onto a builtin `<Ctor>.prototype` used to read
// the key through the STRING payload offsets — a symbol cell has
// different bytes there, so the length and data pointer were garbage
// and the compare walked off the heap. Invisible to every gate we run
// (the read lands inside a live mapping often enough that the output
// stays correct); only Guard Malloc turns it into a crash, and only
// the AOT artifact — `tr run` happened to survive it.
//
// Reached from test262's `staging/sm/Array/concat-spreadable-primitive`
// via `Object.getPrototypeOf(primitive)[Symbol.isConcatSpreadable]`,
// but nothing about it is specific to concat, to `isConcatSpreadable`,
// or to primitives: any symbol key on any prototype did it.
const own = Symbol("own");

const receivers: any[] = [10, false, {}];
for (const value of receivers) {
  const proto: any = Object.getPrototypeOf(value);
  proto[own] = "marked";
  console.log(proto[own]);
  proto[Symbol.isConcatSpreadable] = true;
  console.log(proto[Symbol.isConcatSpreadable]);
  delete proto[own];
  delete proto[Symbol.isConcatSpreadable];
  console.log(proto[own], proto[Symbol.isConcatSpreadable]);
}

// The string-key face on the same receivers has to keep working — the
// builtin method probe is exactly what the symbol guard steps around.
const objProto: any = Object.getPrototypeOf({});
console.log(typeof objProto.hasOwnProperty, typeof objProto.toString);
objProto.zzz = 1;
console.log(objProto.zzz);
delete objProto.zzz;
console.log(objProto.zzz);
// (`({}).zzz` does NOT read it back yet — an own write on
// `Object.prototype` is not visible down the chain. Unrelated to the
// symbol guard here; recorded in plan-state.)
