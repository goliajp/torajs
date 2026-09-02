// 566-04 — a bound function's two print faces read its own name. The
// COMPILER's `__bound_<fn>` wrapper has a fn-name registry row and
// answered both correctly; a cell the any-lane `.bind` mints at
// runtime has no row at all, so `String(f)` answered the anonymous
// native form and `console.log(f)` printed the bare `[Function]` —
// while `f.name` said `"bound add"` the whole time. The name was
// there; nothing was reading it.
//
// §20.2.3.5 leaves the native-code spelling implementation-defined,
// and JSC (so bun) spells it with the name minus ONE `bound `
// marker — one bind's worth. Two binds keep one marker, in both
// faces, which is what makes them agree with the registry-row path
// that already dropped exactly one.
function add(a: number, b: number) { return a + b }

const compiled = add.bind(null);
const runtime: any = (add as any).bind(null);
const twice: any = runtime.bind(null);
const method: any = (Array.prototype.map as any).bind(null);

console.log(JSON.stringify(compiled.name), JSON.stringify(runtime.name));
console.log(JSON.stringify(twice.name), JSON.stringify(method.name));

console.log(JSON.stringify(String(compiled)), JSON.stringify(String(runtime)));
console.log(JSON.stringify(String(twice)), JSON.stringify(String(method)));

console.log(compiled, runtime, twice, method);

// the bound calls still bind, and the partial-application arguments
// still ride
console.log(compiled(1, 2), runtime(3, 4), twice(5, 6));
const plus10: any = (add as any).bind(null, 10);
console.log(plus10(5), JSON.stringify(plus10.name), plus10, JSON.stringify(String(plus10)));
console.log(JSON.stringify(plus10.length), JSON.stringify(runtime.length));
