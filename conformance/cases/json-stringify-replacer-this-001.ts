// §25.5.2.2 step 3 calls the replacer with the HOLDER as its
// receiver, and §25.5.2 step 11 makes the root's holder a synthetic
// `{ "": value }` wrapper — both observable as `this`. The kernel
// already passed them; what was missing was the compile-time census
// entry that promotes an fn-expr in this slot to a `__this` slot.
JSON.stringify({ p: 7 }, function (k: string, v: any) {
  console.log("[" + k + "] this=" + JSON.stringify(this));
  return v;
});

// Nested holders: each property sees the object it lives on.
JSON.stringify({ outer: { inner: 1 } }, function (k: string, v: any) {
  console.log("[" + k + "] " + JSON.stringify(this));
  return v;
});

// An array holder is the array itself.
JSON.stringify([10], function (k: string, v: any) {
  console.log("[" + k + "] " + JSON.stringify(this));
  return v;
});

// A named binding in the slot resolves the same way.
const rep = function (k: string, v: any) {
  if (k !== "") {
    console.log("named [" + k + "] " + JSON.stringify(this));
  }
  return v;
};
JSON.stringify({ q: 1 }, rep);
