// Object.getPrototypeOf over an ns-static value cell — the non-ident
// temp release inline-dropped the immortal cell straight into its
// null drop_fn (RFC 20260731 knife 6: emit_drop_closure now skips
// FLAG_STATIC_LITERAL cells before the dec).
console.log("a", Object.getPrototypeOf(Math.max) === Function.prototype);
console.log("b", Object.getPrototypeOf(Iterator.concat) === Function.prototype);
console.log("c", Object.getPrototypeOf(Object.keys) === Function.prototype);
const m: any = Math.max;
console.log("d", Object.getPrototypeOf(m) === Function.prototype, m(3, 9));
