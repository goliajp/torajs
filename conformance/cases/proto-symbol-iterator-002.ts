// §23.1.3.40 -- Array.prototype[@@iterator] IS the values function.
// Array.prototype is an array exotic object (§23.1.3), so its own
// entries live in the Arr side props, which had no attribute-carrying
// define kernel until now.
const I: any = Symbol.iterator;
const AP: any = Array.prototype;

const x: any = Object.getOwnPropertyDescriptor(AP, I);
console.log(x === undefined ? "MISSING" : typeof x.value + " w=" + x.writable + " e=" + x.enumerable + " c=" + x.configurable);
console.log(AP[I] === AP.values);
console.log(AP[I] === [1][I]);
console.log(Object.getOwnPropertySymbols(AP).map((s: any) => String(s)).indexOf("Symbol(Symbol.iterator)") >= 0);

// the array face still enumerates and iterates
console.log(Object.getOwnPropertyNames(AP).length, Object.keys(AP).length);
console.log(Array.isArray(AP), AP.length);
const a = [1, 2, 3];
let acc = 0;
for (const v of a) acc += v;
console.log(acc, [...a].length, Array.from(a).length);
const it: any = AP[I].call(a);
console.log(it.next().value, it.next().value);

// a plain array's own keys are untouched by the prototype's entry
console.log(Object.getOwnPropertyNames(a).join(","));
console.log(Object.getOwnPropertySymbols(a).length);
