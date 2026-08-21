// `Array<Str>.join` must read each slot by the cell's own layout.
//
// A top-level annotated `const a: string[] = s.split(" ")` is promoted
// to a data global whose slot takes the ANNOTATION's layout (Arr<Str>),
// so `join` dispatches to the owned-Str kernel — while the init filled
// the slots with 32-byte substring views. Read as an owned Str, a view
// answers its parent pointer as the payload: this program printed three
// copies of one garbage character for `a.join("-")`. Element reads and
// `.length` were already correct; only join was wrong, and only under
// `const` + annotation (let / var / unannotated all took the view
// kernel). Pre-existing; found while writing arr-str-elem-drop-001.

const a: string[] = "p q r".split(" ");
console.log(a.join("-"), a.join(""), a.join(", "), a.length, a[1]);

// non-ASCII parent: the view's byte position follows the parent's stride
const u: string[] = "世 界 ab".split(" ");
console.log(u.join("|"), u.length);

// mixed: an owned string pushed next to views in the same array
const m: string[] = "x y".split(" ");
m.push("z" + "w");
console.log(m.join("+"));

// empty pieces and a one-element result
const e: string[] = ",a,,b,".split(",");
console.log(e.join("/"), e.length);
const one: string[] = "solo".split(",");
console.log(one.join("-"));

// the let shape that was already right, as a control
let c: string[] = "p q r".split(" ");
console.log(c.join("-"));
