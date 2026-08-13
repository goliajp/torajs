// §28.1.11 Reflect.ownKeys answers §10.1.11.1's buckets in full:
// integer keys ascending, then string keys in insertion order, then
// symbol keys in insertion order. It used to stop after the strings.
const s1: any = Symbol("one");
const s2: any = Symbol("two");

const o: any = {};
o.b = 1;
o[s1] = 2;
o.a = 3;
o[2] = 4;
o[0] = 5;
o[s2] = 6;

const keys: any = Reflect.ownKeys(o);
console.log(keys.length);
console.log(String(keys[0]));
console.log(String(keys[1]));
console.log(String(keys[2]));
console.log(String(keys[3]));
console.log(String(keys[4]));
console.log(String(keys[5]));

// the two narrower faces still answer their own halves
console.log(Object.getOwnPropertyNames(o).length);
console.log(Object.getOwnPropertySymbols(o).length);
console.log(Object.keys(o).length);

// detached call shape goes the same way
const rk: any = Reflect.ownKeys;
console.log(rk(o).length);

// non-enumerable symbol keys are still own keys
const p: any = {};
Object.defineProperty(p, s1, { value: 1, enumerable: false, configurable: true });
console.log(Reflect.ownKeys(p).length);
console.log(Object.keys(p).length);

// an object with no symbol keys is unchanged
const q: any = { x: 1, y: 2 };
console.log(Reflect.ownKeys(q).length);
console.log(Reflect.ownKeys({}).length);

// arrays keep their length key and gain nothing
const arr: any = [1, 2];
console.log(Reflect.ownKeys(arr).length);
