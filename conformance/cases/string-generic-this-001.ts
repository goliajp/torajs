// §22.1.3 String.prototype methods are generic: a non-string this
// coerces via ToString (observable OrdinaryToPrimitive order).
// test262 trim/15.5.4.20-2-* shape; three coupled pieces — the
// prototype-call desugar skips the String generic family, the
// runtime `.call` re-dispatch coerces (generic_str_this), and
// typevar inference lets `any` absorb.

// full OrdinaryToPrimitive order on a double-object receiver
let toStringAccessed = false;
let valueOfAccessed = false;
const obj: any = {
  toString: function() { toStringAccessed = true; return {}; },
  valueOf: function() { valueOfAccessed = true; return {}; }
};
try {
  (String.prototype.trim as any).call(obj);
  console.log("no throw");
} catch (e: any) {
  console.log("caught:", e instanceof TypeError ? "TypeError" : e);
}
console.log("toString accessed:", toStringAccessed);
console.log("valueOf accessed:", valueOfAccessed);

// hint-string order: toString first, then valueOf
let order: string[] = [];
const o2: any = {
  toString: function() { order.push("toString"); return {}; },
  valueOf: function() { order.push("valueOf"); return {}; }
};
try { (String.prototype.trim as any).call(o2); } catch (e) {}
console.log("order:", order.join(","));

// toString returning a primitive is used
const o3: any = { toString: function() { return "  hi  "; } };
console.log("[" + (String.prototype.trim as any).call(o3) + "]");

// inherited Object.prototype.toString when only valueOf is own
const o4: any = { valueOf: function() { return "  vo  "; } };
console.log("[" + (String.prototype.trim as any).call(o4) + "]");

// number / boolean receivers coerce to their string forms
console.log("[" + (String.prototype.trim as any).call(42) + "]");
console.log("[" + (String.prototype.toUpperCase as any).call(true) + "]");
console.log((String.prototype.charAt as any).call(12345, 2));

// chained form (no variable binding) takes the same runtime lane
var obj2 = { toString: function() { return "  zz  "; } };
console.log(String.prototype.trim.call(obj2));

// string receiver through the chained form still works
console.log("[" + String.prototype.trim.call("  pad  ") + "]");

// direct method on a plain object stays a TypeError (no own method)
try { (({}) as any).trim(); console.log("no throw"); }
catch (e: any) { console.log("direct:", e instanceof TypeError ? "TypeError" : e); }

// undefined receiver stays a TypeError
try { (String.prototype.trim as any).call(undefined); console.log("no throw"); }
catch (e: any) { console.log("nullish:", e instanceof TypeError ? "TypeError" : e); }

// apply lane shares the coerce (substring is a str-only mid; slice
// is shared with the Array surface and stays on the ordinary lane —
// recorded per-family-cell boundary)
console.log("[" + (String.prototype.substring as any).apply(9876, [1, 3]) + "]");
