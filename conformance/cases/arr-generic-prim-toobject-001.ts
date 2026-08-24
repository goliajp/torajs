// §23.1.3 step 1 over bool/number primitives — ToObject mints the
// wrapper as the scan host: own-miss length/index reads walk to the
// wrapper-prototype expando face and %Object.prototype%, and the
// callback's O argument is the wrapper (obj instanceof Boolean).
var bp: any = Boolean.prototype;
bp[0] = 11;
bp[1] = 12;
bp.length = 2;
function isBool(val, idx, o) { return o instanceof Boolean; }
console.log(Array.prototype.every.call(false, isBool));
console.log(Array.prototype.map.call(true, function (v) { return v * 2; }));
console.log(Array.prototype.indexOf.call(false, 12));

var np: any = Number.prototype;
np[0] = "n0";
np.length = 1;
function isNum(val, idx, o) { return o instanceof Number; }
console.log(Array.prototype.some.call(2.5, isNum));
console.log(Array.prototype.join.call(7, "-"));

// own wrapper expando shadows the prototype face
var w: any = new Boolean(true);
w.length = 1;
w[0] = "own";
console.log(Array.prototype.indexOf.call(w, "own"));
console.log(Array.prototype.indexOf.call(w, 12));

// %Object.prototype% root stays reachable behind the wrapper proto
delete bp[1];
(Object.prototype as any)[1] = "root1";
console.log(Array.prototype.indexOf.call(false, "root1"));
