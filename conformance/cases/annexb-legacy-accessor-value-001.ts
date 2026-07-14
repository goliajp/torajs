// Annex B §B.2.2.2-5 — every object inherits __defineGetter__ /
// __defineSetter__ / __lookupGetter__ / __lookupSetter__ from
// Object.prototype. tr answered the CALLS but not the READS: `typeof
// o.__defineGetter__` was undefined, so `.call()` / `.apply()` on one
// (which is how test262 reaches them) never got off the ground.
const o: any = { a: 1 };

console.log(typeof o.__defineGetter__, typeof o.__defineSetter__);
console.log(typeof o.__lookupGetter__, typeof o.__lookupSetter__);
console.log(typeof (Object.prototype as any).__defineGetter__);

// Read one off the receiver and invoke it through .call().
const dg = o.__defineGetter__;
dg.call(o, "v", function () {
  return 42;
});
console.log(o.v);

const lg = o.__lookupGetter__;
console.log(typeof lg.call(o, "v"), lg.call(o, "a"), lg.call(o, "nope"));

// The setter face, same way.
let stored = 0;
o.__defineSetter__("w", function (x: any) {
  stored = x;
});
o.w = 7;
console.log(stored, typeof o.__lookupSetter__("w"));

// A plain data property has neither face.
console.log(o.__lookupGetter__("a"), o.__lookupSetter__("a"));
