// S330 — `Number.parseFloat(Any)` widen + Any→Str decode. ES §19.2.5.2
// step 1 calls ToString(string) which already coerces arbitrary-typed
// input. Pre-S330 check.rs rejected `r_ty: Any` strictly (must be
// String|Undefined), blocking `Number.parseFloat(o as any)` outright.
// Sister to S327 (parseInt Any radix).
//
// ssa_lower mirror decodes Any via anyv_to_str_pair (tag + value)
// before passing to num_parse_float helper.

// Any-typed string
const o1: any = "3.14";
const a = Number.parseFloat(o1);
console.log("a=", a);

// Any from a fn return
const giveStr = (): any => "2.718";
const b = Number.parseFloat(giveStr());
console.log("b=", b);

// Any wrapping a numeric value — ToString → "5" → parseFloat → 5
const o2: any = 5;
const c = Number.parseFloat(o2);
console.log("c=", c);

// Literal string still works (Str fast path, no Any decode)
const d = Number.parseFloat("0.5");
console.log("d=", d);
