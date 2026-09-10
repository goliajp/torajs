// §13.10.2 step 2 reaches GetMethod, whose step 4 throws a TypeError
// when the property is present but not callable. A primitive handler
// fell through to the ordinary prototype walk and answered `false`
// where every engine throws; a non-callable OBJECT handler already
// raised inside the call path.
const C: any = function () {};
Object.defineProperty(C, Symbol.hasInstance, { value: 1 });
try {
  console.log({} instanceof C);
} catch (e: any) {
  console.log("caught", e.constructor.name);
}
// `undefined` / `null` are the two GetMethod reads as "no method":
// the ordinary walk answers, and it is the one §20.2.3.6 owns.
const D: any = function () {};
Object.defineProperty(D, Symbol.hasInstance, { value: null });
console.log(new D() instanceof D, {} instanceof D);
// A callable handler runs, wherever it sits.
const E: any = function () {};
Object.defineProperty(E, Symbol.hasInstance, { value: (v: any) => v.tag === 7 });
console.log({ tag: 7 } instanceof E, {} instanceof E);
const o: any = { [Symbol.hasInstance](v: any) { return true; } };
console.log({} instanceof o);
