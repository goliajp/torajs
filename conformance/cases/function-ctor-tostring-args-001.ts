// CreateDynamicFunction passes every argument through ToString before
// assembling the source text (spec 20.2.1.1), so a non-string literal
// is a legal body or parameter-list text: new Function(undefined) is a
// function whose body text is "undefined".
const f = new Function(undefined);
console.log(typeof f, f());

const g = Function(null);
console.log(typeof g, g());

const h = Function("x", 5);
console.log(typeof h, h.length, h(1));

const b = Function(true);
console.log(typeof b);

const n = Function(42);
console.log(typeof n, n());
