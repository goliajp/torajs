// §10.1.9.2 OrdinarySet consults the user [[Prototype]] chain (RFC
// 20260721 候补刀): inherited setter runs with the original
// receiver; inherited non-writable data / getter-only accessor
// reject; a writable chain answer shadow-creates on the receiver.
var data: any = "NAME";
var ap: any = {};
Object.defineProperty(ap, "nm", {
  get: function () { return data; },
  set: function (v: any) { data = v; },
  enumerable: true,
  configurable: false,
});
var tm: any = Object.create(Object.create(ap));
tm.nm = "Team Meeting";
console.log("setter-through:", data, "own?", tm.hasOwnProperty("nm"), "read:", tm.nm);
var base: any = {};
Object.defineProperty(base, "ro", { value: 1, writable: false });
var child: any = Object.create(base);
try {
  child.ro = 2;
  console.log("ro NO-THROW", child.ro);
} catch (e) {
  console.log("ro threw:", e instanceof TypeError, child.ro, child.hasOwnProperty("ro"));
}
var base2: any = { w: 1 };
var child2: any = Object.create(base2);
child2.w = 5;
console.log("shadow create:", child2.w, base2.w, child2.hasOwnProperty("w"));
var base3: any = {};
Object.defineProperty(base3, "g", { get: function () { return 9; } });
var child3: any = Object.create(base3);
try {
  child3.g = 3;
  console.log("g NO-THROW", child3.g);
} catch (e) {
  console.log("g threw:", e instanceof TypeError, child3.g);
}
