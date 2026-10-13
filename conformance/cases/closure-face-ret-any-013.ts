// A receiver of a union of builtin types, which the checker types
// `any`: a field its library declares answers that (`length` is a
// number), while a method that answers the receiver's own value or an
// array's element answers a value of any of the union's kinds, which a
// function declared `number` hands back as is.
function own(v: string | number): number { return v.valueOf() as any; }
function cast(v: string | number): number { return (v as any).valueOf(); }
function len(v: string | any[]): number { return v.length; }
function first(v: string | any[]): number { return v.at(0); }
function joined(v: string | string[]): string {
  if (Array.isArray(v)) return v.join("+");
  return v;
}
console.log(own("a"), cast("b"), len("abc"), len([1, 2]), first(["q"]), first("r"), joined(["x", "y"]));
