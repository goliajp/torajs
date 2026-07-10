// Chunk 807 — undefined / null array-literal elements without a
// String sibling route through the FLAG_ARR_ANY lane (tagged
// ANY_UNDEF / ANY_NULL slots). Pre-fix: `[undefined]` / `[null]`
// printed `[unknown-any-tag]` (kind-less typed block), and a scalar
// anchor (`[undefined, 1]`) mixed raw ints with null slots and
// SIGSEGV'd in the print walk. The String-sibling shapes keep the
// T-10.c typed Str sentinel lane.

// pure undefined / null
const a1 = [undefined];
console.log(a1);
console.log(a1[0] === undefined, a1.length);
const a2 = [null];
console.log(a2);
console.log(a2[0] === null);

// scalar anchor (pre-fix SIGSEGV)
const a3 = [undefined, 1];
console.log(a3);
console.log(a3[0], a3[1]);

// mixed with search
const a4 = [null, undefined, 2, true];
console.log(a4);
console.log(a4.indexOf(undefined), a4.indexOf(null));

// undefined binding element
const u = undefined;
console.log([u]);

// void-call element
function v(m: string) { console.log(m) }
console.log([v("a")]);

// nested array sibling
console.log([undefined, [1, 2]]);

// String-sibling shapes keep the typed sentinel lane
const s1 = ["a", undefined];
console.log(s1);
console.log(s1[1] === undefined);
const s2 = ["a", null];
console.log(s2);
console.log(s2[1] === null);
