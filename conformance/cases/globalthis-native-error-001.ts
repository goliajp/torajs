// globalThis Error-family fill — the dynamic lane answers the bare
// name's class-object identity, and construct / instanceof / message
// / name all work through a dynamically read constructor.
const g: any = globalThis;
console.log(g.Error === Error);
console.log(g.TypeError === TypeError);
console.log(g.RangeError === RangeError);
console.log(g.ReferenceError === ReferenceError);
console.log(g.SyntaxError === SyntaxError);
console.log(g.EvalError === EvalError);
console.log(g.URIError === URIError);
console.log(g.AggregateError === AggregateError);
console.log(typeof g.TypeError, typeof g.EvalError, typeof g.URIError);
const T: any = g.TypeError;
const e = new T("boom");
console.log(e instanceof TypeError, e instanceof Error);
console.log(e.message, e.name);
const R: any = g.RangeError;
const r = new R("out");
console.log(r instanceof RangeError, r.message);
console.log(globalThis.SyntaxError === SyntaxError);
