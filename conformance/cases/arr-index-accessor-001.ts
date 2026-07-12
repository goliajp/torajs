// RFC 20260713-defprop-residual-cluster chunk C — accessor properties
// on array indexes: the AccessorPair lives in the index's shadow
// entry, element reads route through the getter (get_any_tag caches
// the product for the paired value read), writes through the setter,
// gOPD reports the accessor faces, and a data redefine converts back.
var arr = [];
var stored = 0;
Object.defineProperty(arr, "0", {
  get: function () {
    return 11;
  },
  set: function (v) {
    stored = v;
  },
  configurable: true,
});
console.log(arr[0]);
arr[0] = 42;
console.log(stored);
var d = Object.getOwnPropertyDescriptor(arr, "0");
console.log(typeof d.get, typeof d.set, d.enumerable, d.configurable);
console.log(arr.length);

// Getter-only accessor: assignment throws (module strict mode).
var brr = [];
Object.defineProperty(brr, "0", {
  get: function () {
    return 5;
  },
  configurable: true,
});
try {
  brr[0] = 1;
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}

// Accessor -> data redefine on a configurable index.
Object.defineProperty(arr, "0", { value: 7 });
console.log(arr[0]);
var d2 = Object.getOwnPropertyDescriptor(arr, "0");
console.log(d2.value, d2.writable, d2.enumerable, d2.configurable);

// Fresh accessor via defineProperties on an index past the length.
var crr = [];
Object.defineProperties(crr, {
  "1": {
    get: function () {
      return 33;
    },
    enumerable: true,
  },
});
console.log(crr[1], crr.length, crr[0]);
