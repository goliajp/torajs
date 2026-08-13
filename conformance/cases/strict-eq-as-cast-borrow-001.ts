// `x as any` is value-transparent: for a heap source the cast passes
// the pointer straight through, so the binding still owns it. The
// BinOp drop pass judged ownership off the As node instead of the
// expression under it, read `b as any` as a freshly-allocated temp,
// and released the binding's only stake -- `b` was freed here and its
// scope drop then read a dead header. Nothing in the output moved,
// which is why the gate stayed green; Guard Malloc is what saw it.
const a = Symbol("x");
const b = Symbol("y");
const o: any = a;

console.log(o === (b as any));
console.log(o === (a as any));

// the operands must still be alive and unchanged afterwards
console.log(String(b));
console.log(String(a));
console.log(a === (a as any));

// the same shape one level down, and through a heap string, whose
// drop is the same dispatcher
const s = "abcdefghijklmnopqrstuvwxyz0123456789";
const t: any = s;
console.log(t === (s as any), s.length);

// a genuinely fresh temp on the right still has to be released --
// this pins that the drop was made conditional, not deleted
console.log(t === (String(s) as any), s.length);
