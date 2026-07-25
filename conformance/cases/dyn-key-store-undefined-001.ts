// Storing `undefined` through a dynamic key keeps it `undefined`, not
// `null`.
//
// SSA folds frontend `undefined` and frontend `null` into the same
// `Type::Ptr` / `ConstPtrNull` (RFC 20260725-ssa-undefined-vs-null), so
// the any-slot pair packer — which sees only the SSA type — could
// answer nothing but ANY_NULL for both. `o[k] = undefined` with `k` a
// variable therefore read back as `null`, and `typeof` said "object".
// The literal-key and dot forms were already right because they resolve
// through the member-assign lane, which still holds the frontend type,
// and `Object.defineProperty` was right because it packs elsewhere.
//
// The checker does distinguish the two, and the share-aware pack
// wrapper is handed the value's ExprId, so the undefined case is
// settled there — before the collapsed SSA type is all that is left.
// Both dynamic lanes (string key and §6.1.7 symbol key) go through that
// wrapper, so they cannot disagree with each other.
//
// This is the same root cause the RFC was filed for over
// `JSON.stringify`, showing a second and more everyday face.

const k = "x";
const s = Symbol("s");

const o: any = {};
o[k] = undefined;
console.log("str-key", typeof o[k], o[k]);
console.log("str-key-desc", typeof Object.getOwnPropertyDescriptor(o, "x").value);
o[s] = undefined;
console.log("sym-key", typeof o[s], o[s]);
console.log("sym-key-desc", typeof Object.getOwnPropertyDescriptor(o, s).value);

// `null` must still be null — the two are not interchangeable, which is
// the whole point
o[k] = null;
console.log("str-null", typeof o[k], o[k]);
o[s] = null;
console.log("sym-null", typeof o[s], o[s]);

// a binding holding undefined, not just the literal
const u = undefined;
const viaVar: any = {};
viaVar[k] = u;
console.log("via-var", typeof viaVar[k], viaVar[k]);

// the Array<Any> element slot packs through the same wrapper
const arr: any[] = [1, 2];
arr[0] = undefined;
console.log("arr-elem", typeof arr[0], arr[0], JSON.stringify(arr), arr.length);
arr[1] = null;
console.log("arr-null", typeof arr[1], JSON.stringify(arr));

// the lanes that were already correct stay correct
const lit: any = {};
lit["spelled"] = undefined;
lit.dotted = undefined;
console.log("lit", typeof lit["spelled"], "dot", typeof lit.dotted);
console.log("lit-json", JSON.stringify(lit), JSON.stringify(Object.keys(lit)));

// an undefined value is still a PRESENT property (§10.1.9 creates the
// slot), which is what distinguishes it from an absent key
const present: any = {};
present[k] = undefined;
console.log("present", "x" in present, JSON.stringify(Object.keys(present)));
console.log("absent", "nope" in present, typeof present["nope"]);
