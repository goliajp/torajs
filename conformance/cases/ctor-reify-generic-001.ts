// RFC 20260721-string-proto-cluster 刀 4 (G3) — typed receivers'
// `.constructor` reads reify to the interned builtin ctor value
// (was ConstPtrNull, so any comparison through a generic slot
// answered false; only the AST-level fold of the direct
// `x.constructor === Ctor` spelling worked).
function same(a: any, b: any): boolean {
  return a === b;
}
let arr = "a-b".split("-");
console.log(same(arr.constructor, Array));
console.log(arr.constructor === Array);
let s = "abc";
console.log(same((s as any).constructor, String));
console.log(same([1, 2].constructor, Array));
let n = 5;
console.log(same((n as any).constructor, Number));
let b = true;
console.log(same((b as any).constructor, Boolean));
let x: any = "abc";
console.log(same(x.constructor, String));
console.log(same(x.constructor, Array));
