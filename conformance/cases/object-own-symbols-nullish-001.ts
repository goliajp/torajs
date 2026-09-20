// ES §20.1.2.10 step 1 is ToObject, and ToObject(null) /
// ToObject(undefined) throws a TypeError.
//
// The typed arm here reasons about what a CELL holds -- a symbol key
// can only arrive through defineProperty or a computed key, both of
// which degrade the binding to the dynobj lane first, so a still-typed
// cell provably holds none. That argument is sound for cells and says
// nothing about nullish: a nullish value is not a cell and never
// reaches the question. It took the compile-time empty array anyway,
// so this answered [] where every engine throws -- a wrong answer
// rather than a refusal, which is why it sat in the bugs bucket.
try { Object.getOwnPropertySymbols(undefined); console.log("no-throw"); } catch (e) { console.log("undefined threw"); }
try { Object.getOwnPropertySymbols(null); console.log("no-throw"); } catch (e) { console.log("null threw"); }

// the any spelling has always raised it -- the two must agree
const z: any = null;
try { Object.getOwnPropertySymbols(z); console.log("no-throw"); } catch (e) { console.log("any-null threw"); }
const u: any = undefined;
try { Object.getOwnPropertySymbols(u); console.log("no-throw"); } catch (e) { console.log("any-undefined threw"); }

// every non-nullish receiver keeps the compile-time empty array
console.log(JSON.stringify(Object.getOwnPropertySymbols({ a: 1 })));
console.log(JSON.stringify(Object.getOwnPropertySymbols(1)));
console.log(JSON.stringify(Object.getOwnPropertySymbols("ab")));
console.log(JSON.stringify(Object.getOwnPropertySymbols([])));
console.log(JSON.stringify(Object.getOwnPropertySymbols(true)));
