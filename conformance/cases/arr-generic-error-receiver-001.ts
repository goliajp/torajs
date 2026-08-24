// arraylike_len over a Tag::Obj receiver whose `length` lives in the
// +24 expando dict (an Error instance's layout never spells it) —
// the layout-only read answered ToLength 0, keeping every/some
// vacuously right while indexOf/join/includes read the wrong
// emptiness.
var e: any = new Error("m");
e.length = 2;
e[0] = 11;
e[1] = "x";
console.log(Array.prototype.every.call(e, function (v) { return v !== undefined; }));
console.log(Array.prototype.indexOf.call(e, "x"));
console.log(Array.prototype.lastIndexOf.call(e, 11));
console.log(Array.prototype.includes.call(e, "x"));
console.log(Array.prototype.join.call(e, ","));
console.log(Array.prototype.map.call(e, function (v) { return typeof v; }));
