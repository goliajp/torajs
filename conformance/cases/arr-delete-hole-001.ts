// RFC 20260713-defprop-tpd-cluster chunk C — `delete arr[i]` minimal
// hole semantics. Pre-fix the any-world delete on a canonical index
// was a silent no-op (claimed success, element stayed own) — the
// dominant blocker for test262 propertyHelper's isConfigurable probe
// (verifyProperty on any array index). Element storage stays dense;
// a HOLE shadow entry marks the index absent for every own-property
// consumer.

let arr = [];
arr[0] = 100;
arr[1] = 200;
arr[2] = 300;
var o: any = arr;

// delete → absent everywhere, length unchanged
delete o["1"];
console.log("hasOwn:", o.hasOwnProperty("1"), "elem:", o[1], "len:", o.length);
console.log("keys:", Object.keys(o).join(","));
console.log("gOPN:", Object.getOwnPropertyNames(o).join(","));
var d: any = Object.getOwnPropertyDescriptor(o, "1");
console.log("gOPD:", d === undefined);
var seen = "";
for (var k in o) seen += k + ";";
console.log("forin:", seen);
console.log("pie:", o.propertyIsEnumerable("1"));

// plain write revives as a fresh default data property
o[1] = 555;
console.log("revive:", o[1], o.hasOwnProperty("1"), Object.keys(o).join(","));

// defineProperty revives too (fresh create, no current-flags validate)
delete o["1"];
Object.defineProperty(o, "1", { value: 7, enumerable: true, writable: true, configurable: true });
console.log("defrevive:", o[1], o.hasOwnProperty("1"), o.propertyIsEnumerable("1"));

// non-configurable index refuses the delete (strict TypeError)
Object.defineProperty(o, "2", { configurable: false });
var threw = false;
try {
  delete o["2"];
} catch (e) {
  threw = true;
}
console.log("nc-refuse:", threw, o.hasOwnProperty("2"), o[2]);

// delete is idempotent; out-of-range delete answers true silently
delete o["1"];
delete o["1"];
delete o["99"];
console.log("idem:", o.hasOwnProperty("1"), o.length);

// length delete refuses
var threw2 = false;
try {
  delete o.length;
} catch (e) {
  threw2 = true;
}
console.log("len-refuse:", threw2, o.length);
