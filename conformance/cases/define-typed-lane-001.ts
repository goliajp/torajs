// define 家族 typed-lane 路由 (RFC 20260721-object-descriptor-cluster
// 刀 2) — pre-fix these silently no-opped:
// 1. defineProperty on a typed Closure receiver (§8.12.9 via the
//    kernel closure-expando arm), incl. non-configurable redefine
//    rejection.
// 2. defineProperties / Object.create with a typed Closure / Array
//    props argument (§20.1.2.3.1 walks its expando own props).
// 3. defineProperties with a typed struct / Closure descObj member
//    (§6.2.6.5 ToPropertyDescriptor reads off ANY object shape).
var fun = function () {};
Object.defineProperty(fun, "foo", { value: 12, configurable: false });
console.log((fun as any).foo);
try {
  Object.defineProperty(fun, "foo", { value: 11, configurable: true });
  console.log("no-throw");
} catch (e: any) {
  console.log("redefine:", e instanceof TypeError);
}
console.log((fun as any).foo);

var fun2 = function () {};
Object.defineProperties(fun2, { prop: { value: 11 } });
console.log((fun2 as any).prop, (fun2 as any).hasOwnProperty("prop"));

var obj: any = {};
var props = function () {};
Object.defineProperty(props, "prop", {
  value: { value: 7 },
  enumerable: true,
});
Object.defineProperties(obj, props);
console.log(obj.hasOwnProperty("prop"), obj.prop);

var obj2: any = {};
var arrProps: any[] = [];
var accessed = false;
Object.defineProperty(arrProps, "prop", {
  get: function () {
    accessed = this instanceof Array;
    return { value: 9 };
  },
  enumerable: true,
});
Object.defineProperties(obj2, arrProps);
console.log(accessed, obj2.prop.value);

var created: any = Object.create({}, props);
console.log(created.hasOwnProperty("prop"), created.prop);

var obj3: any = {};
var descObj = { value: 101, enumerable: true };
Object.defineProperties(obj3, { named: descObj });
console.log(obj3.named, obj3.hasOwnProperty("named"));

var obj4: any = {};
var descFn = function () {};
(descFn as any).value = 202;
Object.defineProperties(obj4, { fromFn: descFn });
console.log(obj4.fromFn);
