// An any-typed string receiver runs the same symbol dispatch as the
// typed one: match §22.1.3.12 step 2, search §22.1.3.20 step 2,
// replace §22.1.3.19 step 2 and replaceAll §22.1.3.20 step 2.c call a
// user @@match / @@search / @@replace before coercing anything. A
// non-callable method is the GetMethod TypeError, a throwing one
// propagates, and a ToString that throws on the fallback lane stops
// the call.

var s: any = "abc";
var seen: any[] = [];

var om: any = {};
om[Symbol.match] = function (x: string) {
  seen.push(this === om);
  return "M:" + x;
};
var os: any = {};
os[Symbol.search] = function (x: string) {
  seen.push(this === os);
  return "S:" + x;
};
var orp: any = {};
orp[Symbol.replace] = function (x: string, r: any) {
  seen.push(this === orp);
  return "R:" + x + ":" + r;
};
console.log(s.match(om), s.search(os), s.replace(orp, "Z"), s.replaceAll(orp, 7));
console.log(seen.join(","));

// the builtin faces keep their kernels
console.log(s.match(/b/)[0], s.search(/c/), s.replace(/b/, "X"), s.replaceAll(/b/g, "Y"));
console.log(s.match("b")[0], s.search("c"), s.replace("b", "X"), s.replaceAll("c", "W"));

// a nullish method falls back to the coerce lane
var nul: any = {};
nul[Symbol.match] = undefined;
nul[Symbol.search] = null;
nul.toString = function () {
  return "c";
};
console.log(s.match(nul)[0], s.search(nul));

var bad: any = {};
bad[Symbol.match] = 1;
bad[Symbol.search] = "x";
bad[Symbol.replace] = {};
try {
  s.match(bad);
  console.log("no-throw");
} catch (e) {
  console.log("match", e instanceof TypeError);
}
try {
  s.search(bad);
  console.log("no-throw");
} catch (e) {
  console.log("search", e instanceof TypeError);
}
try {
  s.replace(bad, "z");
  console.log("no-throw");
} catch (e) {
  console.log("replace", e instanceof TypeError);
}

class Boom extends Error {}
var thr: any = {};
thr[Symbol.search] = function () {
  throw new Boom("searcher");
};
try {
  s.search(thr);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof Boom, (e as any).message);
}

var ts: any = {
  toString() {
    throw new Boom("tostring");
  },
};
try {
  s.match(ts);
  console.log("no-throw");
} catch (e) {
  console.log("match", (e as any).message);
}
try {
  s.search(ts);
  console.log("no-throw");
} catch (e) {
  console.log("search", (e as any).message);
}
