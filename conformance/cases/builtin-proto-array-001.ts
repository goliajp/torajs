// ES §23.1.3 — `Array.prototype` is an Array exotic object (an empty
// one), not an ordinary object. Every array-shaped answer below comes
// from that, and the own-property surface has to keep working on a
// cell that is no longer a dynobj.
const AP: any = Array.prototype;

// The array face.
console.log(Array.isArray(AP), AP.length, "[" + AP.toString() + "]");
console.log(Object.prototype.toString.call(AP));
console.log("[" + AP.join("-") + "]", AP.indexOf(1), AP.slice().length);
console.log(AP.concat([1, 2]).length);
let seen = 0;
AP.forEach(() => {
  seen++;
});
console.log("forEach:", seen);

// §10.1.1 — the prototype chain around it. A prototype is not on its
// own chain, so it is not an instance of its constructor.
console.log(Object.getPrototypeOf(AP) === Object.prototype);
console.log(Object.getPrototypeOf([1]) === Array.prototype);
console.log(AP instanceof Array);
console.log(Array.prototype === Array.prototype);

// Own properties: a monkey-patch lands on the cell, reads back through
// both the static and the dynamic lane, and shows up in `in` / gOPD.
Array.prototype.zap = function () {
  return "zap";
};
console.log(typeof (Array.prototype as any).zap, "zap" in AP);
console.log(typeof Object.getOwnPropertyDescriptor(AP, "zap"));
console.log(Object.prototype.hasOwnProperty.call(AP, "zap"));

// The interned family methods are own properties too, with no entry
// backing them — and `delete` tombstones them for every reader.
console.log(typeof (Array.prototype as any).map, "map" in AP);
console.log(typeof Object.getOwnPropertyDescriptor(AP, "map"));
console.log(delete (Array.prototype as any).map, "map" in AP);

// The primitive-data prototypes keep their own shape (rotation 101).
console.log(Number.prototype.toString(), Boolean.prototype.toString());
