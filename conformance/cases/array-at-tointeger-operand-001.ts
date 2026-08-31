// §23.1.3.1 step 2 is ToIntegerOrInfinity(index), which reaches every
// value. The Array lane admitted Number and Undefined by name and
// refused the rest; the String siblings were turned into the coercion
// the spec step actually is by rotation 463, and this is the Array
// half of that.

const one: any = 1;
const oneStr: any = "1";
const undef: any = undefined;
const obj: any = {};

console.log([9, 8, 7].at(one));
console.log([9, 8, 7].at(oneStr));
console.log([9, 8, 7].at(undef));
console.log([9, 8, 7].at(one, 0));
console.log(["x", "y"].at(one));
console.log([9, 8, 7].at(obj));

// Statically-shaped operands take the same coercion.
console.log([9, 8, 7].at("1"));
console.log([9, 8, 7].at(true));
console.log([9, 8, 7].at(null));
console.log([9, 8, 7].at(NaN));
console.log([[1], [2]].at("1")[0]);

// The typed tier and the arity defaults are unchanged.
console.log([9, 8, 7].at(-1), [9, 8, 7].at(1.7), [9, 8, 7].at(9));
console.log([9, 8, 7].at());
console.log([9, 8, 7].at(undefined));

// A trailing arg is still evaluated and discarded.
let k = 0;
console.log([9, 8, 7].at(1, k++), k);
