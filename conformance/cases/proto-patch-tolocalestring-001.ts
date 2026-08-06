// §20.1.3.5 — `Object.prototype.toLocaleString` is `Invoke(this,
// "toString")`, so on every family that INHERITS it a patched
// `<Ctor>.prototype.toString` shows through. Only the families that
// redefine the property (Number / Array / Date / BigInt, ECMA-402
// §18-20) are opaque to such a patch.
function owns(proto: any, name: string): string {
  return "" + Object.prototype.hasOwnProperty.call(proto, name);
}

console.log("own Number=" + owns(Number.prototype, "toLocaleString"));
console.log("own String=" + owns(String.prototype, "toLocaleString"));
console.log("own Boolean=" + owns(Boolean.prototype, "toLocaleString"));
console.log("own Array=" + owns(Array.prototype, "toLocaleString"));
console.log("own Date=" + owns(Date.prototype, "toLocaleString"));
console.log("own Map=" + owns(Map.prototype, "toLocaleString"));
console.log("own Function=" + owns(Function.prototype, "toLocaleString"));
console.log("own Symbol=" + owns(Symbol.prototype, "toLocaleString"));

// Reading it still resolves everywhere — inheriting a property is not
// the same as not having one.
console.log("read String=" + typeof (String.prototype as any).toLocaleString);
console.log("read Function=" + typeof (Function.prototype as any).toLocaleString);

function seen(recv: any, proto: any): string {
  const orig: any = proto.toString;
  proto.toString = function () { return "T"; };
  let out = "";
  try { out = recv.toLocaleString() === "T" ? "SEEN" : "NOT-SEEN"; } catch (e: any) { out = "THROW"; }
  proto.toString = orig;
  return out;
}

// Own toLocaleString — the patch must not show through.
console.log("num " + seen(5, Number.prototype));
console.log("arr " + seen([1, 2], Array.prototype));
console.log("date " + seen(new Date(0), Date.prototype));

// Inherited toLocaleString — the patch is the answer.
console.log("str " + seen("ab", String.prototype));
console.log("bool " + seen(true, Boolean.prototype));
console.log("map " + seen(new Map(), Map.prototype));
console.log("set " + seen(new Set(), Set.prototype));
console.log("regexp " + seen(/a/, RegExp.prototype));
console.log("symbol " + seen(Symbol("x"), Symbol.prototype));

// Deleting a family's own toLocaleString puts it back on the
// inherited one, so the leg starts running for it.
const origNumTLS: any = (Number.prototype as any).toLocaleString;
delete (Number.prototype as any).toLocaleString;
console.log("num-after-delete " + seen(5, Number.prototype));
(Number.prototype as any).toLocaleString = origNumTLS;
