// A multi-arg `console.log` in VALUE position — an expression-bodied
// arrow's body, an assignment's right side — inspects its arguments
// exactly as the statement form does. It used to take the Str joiner
// (`console.error`'s shape), so an object printed as `[object Object]`.
class S { value: number; done: boolean; constructor() { this.value = 1; this.done = true; } }
const obj = { value: 1, done: true };
const arr = [1, 2, 3];
const logExpr = (tag: string, r: any) => console.log(tag, r, r.done);
logExpr("expr-obj", obj);
logExpr("expr-struct", new S());
const logOne = (r: any) => console.log(r);
logOne(obj);
logOne(arr);
const v: void = console.log("assigned", obj, arr, 1.5, "s", true, null, undefined);
Promise.resolve(obj).then((r: any) => console.log("then-expr", r));
Promise.resolve(new S()).then((r: any) => console.log("then-struct", r, r.value));
async function* g() { yield 0; }
const it = g();
it.next().then((r: any) => console.log("step", r, r.done));
