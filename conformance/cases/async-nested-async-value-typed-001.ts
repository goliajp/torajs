// An async function VALUE with no await in it is not moved into a state
// machine; `desugar_async` rewrites it later, and its return becomes
// `Promise<T>`. A machine local holding one is typed as that later
// value — the field used to take the body's own return (`() => number`)
// and the assignment failed ("field is Function([], Number), value is
// Function([], Promise(Any))").
function helper(fn: any) { fn(); }
helper(async function () { let m = 1; const h = async function () { return m; }; await null; m = 2; console.log("a", await h()); });
helper(async function () { let m = 1; const h = async () => m; await null; m = 2; console.log("b", await h()); });
async function decl() { let m = 1; const h = async function () { return m; }; await null; m = 2; console.log("c", await h()); }
decl();
helper(async function () { let m = 1; const h = async function (): Promise<number> { return m; }; await null; m = 2; console.log("d", await h()); });
helper(async function () { let m = 1; const h = async function (): number { return m; }; await null; m = 2; const r: any = await h(); console.log("e", r); });
