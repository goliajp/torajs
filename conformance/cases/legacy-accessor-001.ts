// RFC 20260713-annexb-legacy-accessor — Object.prototype
// __defineGetter__ / __defineSetter__ / __lookupGetter__ /
// __lookupSetter__ (Annex B §B.2.2.2-5).

// define + read through the getter
var o: any = {};
o.__defineGetter__("g", function () {
  return 41 + 1;
});
console.log("g =", o.g);

// setter face + assignment
let seen: any = null;
o.__defineSetter__("s", function (v: any) {
  seen = v;
});
o.s = 7;
console.log("seen =", seen);

// both faces on ONE key merge into one accessor
var acc: any = {};
let backing = 10;
acc.__defineGetter__("v", function () {
  return backing;
});
acc.__defineSetter__("v", function (nv: any) {
  backing = nv;
});
console.log("v =", acc.v);
acc.v = 99;
console.log("v after set =", acc.v);

// lookup answers the same function object
var fn = function () {
  return 1;
};
var lk: any = {};
lk.__defineGetter__("p", fn);
console.log("lookup identity =", lk.__lookupGetter__("p") === fn);
console.log("lookup other face =", lk.__lookupSetter__("p"));
console.log("lookup miss =", lk.__lookupGetter__("absent"));

// data property answers undefined from lookup
var dt: any = { d: 5 };
console.log("data lookup =", dt.__lookupGetter__("d"));

// non-callable define throws
try {
  o.__defineGetter__("bad", 42);
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}

// throwing getter installed via __defineGetter__ propagates
var th: any = {};
th.__defineGetter__("boom", function () {
  throw new Error("legacy boom");
});
try {
  th.boom;
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}

// builtin reflection
console.log("dg.length =", Object.prototype.__defineGetter__.length);
console.log("ds.name =", Object.prototype.__defineSetter__.name);
console.log("lg.length =", Object.prototype.__lookupGetter__.length);
