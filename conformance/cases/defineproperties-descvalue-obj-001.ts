// RFC 20260716 刀 22 — Object.defineProperties(obj, props) desc-value
// gate. `__torajs_dynobj_define_properties_from` (torajs-dynobj/
// define_all.rs) rejected desc cells with a stricter tag set than
// its own dispatcher (`__torajs_dynobj_define_from_desc`), so an
// ObjectLit-shaped desc value (`{value: {}, enumerable: true}`
// propagated through `Object.defineProperty(props, ...)`) landed
// as `Tag::Obj` and was thrown here as "Property description must
// be an object." — while the dispatcher's `_ => null` fallback
// would have accepted it as an empty descriptor. Test262 shape:
// 15.2.3.7-3-1 / 15.2.3.7-3-4 / 15.2.3.7-3-8. Assertions here only
// probe `hasOwnProperty` — accessor-body invocation on `Tag::Obj`
// desc cells (case C) is a separate substrate gap (L3b), not this
// blade's target.
const objA: any = {};
const propsA: any = {};
Object.defineProperty(propsA, "prop", { value: {}, enumerable: true });
Object.defineProperties(objA, propsA);
console.log("A hasOwn:", objA.hasOwnProperty("prop"));

const objB: any = {};
const propsB: any = {};
Object.defineProperty(propsB, "prop", { value: { configurable: true }, enumerable: true });
Object.defineProperties(objB, propsB);
console.log("B hasOwn:", objB.hasOwnProperty("prop"));

const objD: any = {};
const propsD: any = {};
Object.defineProperty(propsD, "prop", { value: {}, enumerable: false });
Object.defineProperties(objD, propsD);
console.log("D hasOwn:", objD.hasOwnProperty("prop"));
