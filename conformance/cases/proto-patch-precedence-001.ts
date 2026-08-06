// §10.1.8.1 resolves a receiver's own properties before its
// prototype's — so a patch on a builtin prototype may be consulted
// before the native arm exactly where the receiver has no own face to
// come first. That is true of every shape below: a property write to
// a Map / Set / Date / RegExp / weak instance is refused today, so
// their prototype is the only place a method can come from.
//
// Array and Function are deliberately NOT here. They carry expandos
// (`a.push = f` must keep beating `Array.prototype.push = g`), so
// their patch consult stays on the dispatcher's tail until the
// own-resolution boundary inside their arms is mapped out.

const nativeMapGet = (Map.prototype as any).get;
const nativeSetHas = (Set.prototype as any).has;

let log: string = "";

(Map.prototype as any).get = function (k: any) {
  log = log + "get;";
  return "PATCHED";
};
(Set.prototype as any).has = function (v: any) {
  log = log + "has;";
  return false;
};
(Date.prototype as any).getTime = function () {
  log = log + "time;";
  return 7;
};
(RegExp.prototype as any).test = function (s: any) {
  log = log + "test;";
  return false;
};
(WeakMap.prototype as any).get = function (k: any) {
  log = log + "wget;";
  return "W";
};

const m: any = new Map([[1, "native"]]);
const s: any = new Set([1]);
const d: any = new Date(0);
const r: any = /x/;
const wm: any = new WeakMap();
const key: any = {};

console.log("map.get   ", m.get(1));
console.log("set.has   ", s.has(1));
console.log("date.time ", d.getTime());
console.log("regexp    ", r.test("x"));
console.log("weakmap   ", wm.get(key));
console.log("order     ", log);

// the collection still holds what it was built with — the patch
// replaced the read, not the storage
(Map.prototype as any).get = nativeMapGet;
(Set.prototype as any).has = nativeSetHas;
console.log("restored  ", m.get(1), s.has(1), s.has(9));

// an accessor-shaped patch resolves through its getter, with the
// original receiver as `this`
Object.defineProperty(Set.prototype, "has", {
  get: function () {
    return function () {
      return "ACCESSOR";
    };
  },
  configurable: true,
});
console.log("accessor  ", s.has(1));
