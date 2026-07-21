// RFC 20260721 刀 11 G13 — a builtin-prototype monkey-patch (data or
// accessor shape) must consult BEFORE the primitive fast arms answer:
// bool/num receivers directly, per-element through the typed-lane
// Array.prototype.toLocaleString walk, and the §20.1.3.5
// toLocaleString → Invoke(this, "toString") inherited leg.
"use strict";
let pre = ["", ""].toLocaleString();
console.log("pre:", pre === "," ? "comma" : pre);
Object.defineProperty(Boolean.prototype, "toString", {
  get: function () {
    let v = typeof this;
    return function () {
      return v;
    };
  },
});
console.log("bool tls:", [true, false].toLocaleString());
let b: any = true;
console.log("bool direct:", b.toString());
Object.defineProperty(String.prototype, "toLocaleString", {
  get() {
    console.log("getter typeof this:", typeof this);
    return function () {
      return "hooked";
    };
  },
});
console.log("str tls:", ["test"].toLocaleString());
Object.defineProperty(Number.prototype, "toFixed", {
  get: function () {
    return function () {
      return "numpatched";
    };
  },
});
let n: any = 42;
console.log("num direct:", n.toFixed(2));
