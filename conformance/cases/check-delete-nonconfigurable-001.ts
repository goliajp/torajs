// chunk D-2a (RFC 20260711 propertyHelper) — §10.1.10 OrdinaryDelete
// step 4: delete on a non-configurable own property refuses and
// (module-strict, §13.5.1.2) throws a catchable TypeError. The
// propertyHelper isConfigurable probe is exactly this shape.
const o: any = {};
Object.defineProperty(o, "nc", { value: 1, configurable: false });
try {
  delete o.nc;
  console.log("no throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}
console.log(o.hasOwnProperty("nc"), o.nc);
Object.defineProperty(o, "c", { value: 2, configurable: true });
delete o.c;
console.log(o.hasOwnProperty("c"));
const p: any = { plain: 3 };
delete p.plain;
console.log(p.hasOwnProperty("plain"));
console.log("done");
