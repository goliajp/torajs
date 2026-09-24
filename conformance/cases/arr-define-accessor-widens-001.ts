// An array binding the program installs an accessor on is not a
// typed-slot array: Object.defineProperty(a, "0", { get }) must run
// the getter, not be declined silently while reads keep the old slot.
var b = [10, 20, 30];
Object.defineProperty(b, "1", { get() { return 99; } });
console.log(b[1], b.join(), b.reduce((a, v) => a + v, 0));

var seen: any[] = [];
var arr = [1, 2, 3];
Object.defineProperty(arr, "0", {
  get: function () { arr.length = 2; return 0; },
  configurable: true,
});
arr.forEach(function (v) { seen.push(v); });
console.log(seen.join(), arr.length);

var r = [5, 6];
Reflect.defineProperty(r, "1", { set(v: any) { console.log("set", v); }, get() { return 7; } });
r[1] = 8;
console.log(r[1]);

// A data definition keeps the typed slots.
var c = [1, 2];
Object.defineProperty(c, "0", { value: 5, writable: false });
console.log(c[0], c.join());
