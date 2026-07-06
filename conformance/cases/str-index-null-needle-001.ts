// chunk 616 — indexOf-family null-literal needle: checker admits +
// lowering folds ToString(null) = "null" (S235 sibling of the
// undefined widen; search(null) compiles /null/ which is literally
// the same substring probe).
console.log("xnully".indexOf(null));
console.log("abc".indexOf(null));
console.log("xnully".lastIndexOf(null));
console.log("xnully".includes(null));
console.log("abc".includes(null));
console.log("nullz".startsWith(null));
console.log("znull".endsWith(null));
console.log("xnully".search(null));
console.log("xnully".indexOf(undefined));
console.log("xundefinedy".indexOf(undefined));
