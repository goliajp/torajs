// S130-5 narrow: `<v: any> instanceof Object` / `instanceof Function`
// for Any operands. S130-4 added the built-in tag dispatch for
// Array / Date / RegExp / Promise / Map / Set / WeakMap / WeakSet but
// left `Object` / `Function` on the user-class OR-chain path which
// always returns false (built-in tags don't carry class_tag@+8). Per
// JS spec every heap-stored object except the primitive-wrapper tags
// (Tag::Str / Symbol / BigInt) is `instanceof Object`; functions
// instances live on Tag::Closure=3.
class K {
  x: number = 1;
  constructor(x: number) {
    this.x = x;
  }
}
const k = new K(7);
const cl = (x: number) => x;
const dyn = { a: 1 };

const arrAny: any = [1, 2];
const dAny: any = new Date(0);
const rAny: any = /abc/;
const pAny: any = Promise.resolve(1);
const mAny: any = new Map<string, number>();
const sAny: any = new Set<string>();
const wmAny: any = new WeakMap<object, number>();
const wsAny: any = new WeakSet<object>();
const kAny: any = k;
const clAny: any = cl;
const dynAny: any = dyn;

// instanceof Object — every heap-stored object except primitive
// wrappers is `instanceof Object`.
console.log(arrAny instanceof Object);
console.log(dAny instanceof Object);
console.log(rAny instanceof Object);
console.log(pAny instanceof Object);
console.log(mAny instanceof Object);
console.log(sAny instanceof Object);
console.log(wmAny instanceof Object);
console.log(wsAny instanceof Object);
console.log(kAny instanceof Object);
console.log(clAny instanceof Object);
console.log(dynAny instanceof Object);

// instanceof Function — only Tag::Closure cells.
console.log(clAny instanceof Function);
console.log(arrAny instanceof Function);
console.log(kAny instanceof Function);

// Primitives via Any → instanceof Object/Function is false.
const nAny: any = 42;
const sAny2: any = "hi";
console.log(nAny instanceof Object);
console.log(sAny2 instanceof Object);
console.log(nAny instanceof Function);
console.log(sAny2 instanceof Function);

// Symbol / BigInt primitives are stored as heap cells but are still
// JS primitives — instanceof Object must return false. This guard is
// what makes the new Object helper differ from the generic builtin
// tag helper (which would always return true for any heap cell).
const symAny: any = Symbol("hi");
const bigAny: any = 99n;
console.log(symAny instanceof Object);
console.log(bigAny instanceof Object);
