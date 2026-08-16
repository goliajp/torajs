// Bottom of the chain — its body must run before anything that
// requests it (mod-eval-order-001).
console.log("leaf body");
export const L = "L";
