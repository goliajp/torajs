// §6.2.6.5 ToPropertyDescriptor reads its fields with [[Get]], so a
// descriptor object may INHERIT them — `new Con()` with the fields on
// `Con.prototype`, or `Object.create(base)` — through both
// Object.defineProperty and Object.defineProperties (test262
// defineProperties 15.2.3.7-5-b-*).
var proto: any = { value: "inheritedDataProperty" };
var Con: any = function () {};
Con.prototype = proto;
var descObj: any = new Con();
var obj: any = {};
Object.defineProperties(obj, { property: descObj });
console.log(obj.property);
var d = Object.getOwnPropertyDescriptor(obj, "property");
console.log(d.writable, d.enumerable, d.configurable);
var obj2: any = {};
Object.defineProperty(obj2, "p", descObj);
console.log(obj2.p);
// two levels up, and an own field shadowing an inherited one
var base: any = { value: "base", enumerable: true };
var mid: any = Object.create(base);
mid.configurable = true;
var leaf: any = Object.create(mid);
leaf.value = "leaf";
Object.defineProperty(obj2, "q", leaf);
var dq = Object.getOwnPropertyDescriptor(obj2, "q");
console.log(obj2.q, dq.writable, dq.enumerable, dq.configurable);
// accessor descriptor inherited
var gproto: any = { get: function () { return "g"; }, configurable: true };
Object.defineProperty(obj2, "acc", Object.create(gproto));
console.log(obj2.acc, Object.getOwnPropertyDescriptor(obj2, "acc").set);
// an inherited getter on the descriptor field runs with the descriptor as this
var vproto: any = {};
Object.defineProperty(vproto, "value", { get: function (this: any) { return this.tag + "!"; } });
var vdesc: any = Object.create(vproto);
vdesc.tag = "t";
Object.defineProperty(obj2, "v", vdesc);
console.log(obj2.v);
