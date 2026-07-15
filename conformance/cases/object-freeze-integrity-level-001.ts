// RFC 20260716 刀 24 — Object.freeze full integrity-level walk.
// Spec ES §20.1.2.6 = SetIntegrityLevel(O, frozen), which implies
// sealed which implies non-extensible + clears writable AND
// configurable on every own property. Pre-fix tr only set FLAG_FROZEN
// on the header, leaving descriptors intact (isFrozen=true but
// isSealed=false / isExtensible=true / gOPD.writable=true /
// gOPD.configurable=true — silent-wrong across integrity queries).
// test262 cluster: Object/freeze/15.2.3.9-2-{4,a-1,a-4,a-9,a-10,c-1,c-2}.

// A. data property + freeze → writable + configurable both false,
//    enumerable preserved. All three integrity queries answer bun-spec.
const objA: any = {};
objA.foo = 10;
Object.freeze(objA);
const dA = Object.getOwnPropertyDescriptor(objA, "foo");
console.log("A writable:", dA.writable, "configurable:", dA.configurable, "enumerable:", dA.enumerable);
console.log("A isFrozen:", Object.isFrozen(objA), "isSealed:", Object.isSealed(objA), "isExtensible:", Object.isExtensible(objA));

// B. accessor property + freeze → configurable false, enumerable
//    preserved (writable is meaningless on accessors and unread).
const objB: any = {};
Object.defineProperty(objB, "acc", { get: () => 42, enumerable: true, configurable: true });
Object.freeze(objB);
const dB = Object.getOwnPropertyDescriptor(objB, "acc");
console.log("B configurable:", dB.configurable, "enumerable:", dB.enumerable);

// C. pre-existing non-enumerable data prop retains enumerable=false;
//    freeze still clamps writable + configurable to false.
const objC: any = {};
Object.defineProperty(objC, "bar", { value: 5, writable: true, enumerable: false, configurable: true });
Object.freeze(objC);
const dC = Object.getOwnPropertyDescriptor(objC, "bar");
console.log("C writable:", dC.writable, "configurable:", dC.configurable, "enumerable:", dC.enumerable);

// D. empty object + freeze is legal + integrity queries agree.
const objD: any = {};
Object.freeze(objD);
console.log("D isFrozen:", Object.isFrozen(objD), "isSealed:", Object.isSealed(objD), "isExtensible:", Object.isExtensible(objD));

// E. regression sentinel — Object.seal alone still leaves writable
//    intact (seal clears configurable only; frozen fixture must not
//    accidentally teach the sealed walk to clear writable too).
const objE: any = {};
objE.baz = 7;
Object.seal(objE);
const dE = Object.getOwnPropertyDescriptor(objE, "baz");
console.log("E writable:", dE.writable, "configurable:", dE.configurable, "isSealed:", Object.isSealed(objE), "isFrozen:", Object.isFrozen(objE));
