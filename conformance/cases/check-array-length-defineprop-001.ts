// T-29.b — `Object.defineProperty(arr, "length", { value })` validation
// per JS spec §9.4.2.4 ArraySetLength. Throws RangeError if
// ToUint32(value) !== ToNumber(value) (negative, NaN, fractional,
// overflow). Also: descriptors without a `.value` field (just
// writable / configurable / enumerable / get / set) must NOT crash
// the lower — tora's subset doesn't track attribute flags but the
// call shape has to be accepted.

let arr: any[] = [];

// Negative length → RangeError.
let threw1: boolean = false;
try { Object.defineProperty(arr, "length", {value: -1, configurable: true}); }
catch (e: number) { threw1 = true; }
console.log(threw1);  // true

// NaN length → RangeError.
let threw2: boolean = false;
try { Object.defineProperty(arr, "length", {value: NaN, enumerable: true}); }
catch (e: number) { threw2 = true; }
console.log(threw2);  // true

// Number.MAX_SAFE_INTEGER overflows uint32 → RangeError.
let threw3: boolean = false;
try { Object.defineProperty(arr, "length", {value: 9007199254740991, writable: true}); }
catch (e: number) { threw3 = true; }
console.log(threw3);  // true

// Tolerance — descriptor with no `.value`. Must not panic. Silent
// no-op (attribute flags aren't tracked in this phase).
Object.defineProperty(arr, "length", {writable: false});
console.log("tolerance-ok");

// Fractional value → RangeError.
let threw4: boolean = false;
try { Object.defineProperty(arr, "length", {value: 1.5, configurable: true}); }
catch (e: number) { threw4 = true; }
console.log(threw4);  // true
