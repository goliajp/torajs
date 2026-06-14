// `const X: any = [...]` Array literal binding lowers through Arr<Any>
// (per-slot NaN-box AnyValue) instead of the typed Arr<T> fast path,
// so inspect.rs Tag::Arr arm's __torajs_arr_print_any walker decodes
// each 8-byte slot as a proper AnyValue. Without the Any-binding arm
// in LetDecl's init lowering, the outer ANY_HEAP=4 wrapper exposes
// raw 8-byte int slots that the NaN-box decoder reads as
// is_cell(1)=true → deref ptr `1` SIGSEGV.
const a: any = [1, 2, 3];
const b: any = [1.5, 2.5];
const c: any = ["a", "b"];
const d: any = [true, false];
const m: any = [1, "x", true];
console.log(a);
console.log(b);
console.log(c);
console.log(d);
console.log(m);
