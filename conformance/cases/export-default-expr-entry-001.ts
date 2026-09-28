// the entry's own `export default <expr>` evaluates the expression
// (§16.2.1.6.1); nobody reads the binding, the effect still happens in
// statement order
const log: string[] = [];
log.push("before");
export default (log.push("default evaluated"), 42);
log.push("after");
console.log(log.join(","));
