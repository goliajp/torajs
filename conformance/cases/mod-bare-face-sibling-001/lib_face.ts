// A bare export face beside siblings that read the ORIGINAL
// spelling: the face rename must follow every reference (the census
// arena rewrite), not just a fn's self-references.
const a = 5;
export { a as b };
export const c = a * 2;
export function reada() { return a + 100; }
function f() { return f2() + 1; }
function f2() { return 3; }
export { f as g };
