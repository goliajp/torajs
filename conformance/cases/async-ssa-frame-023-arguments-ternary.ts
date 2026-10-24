// An async body without an await that returns a conditional over
// `arguments`: the body runs to its end inside the call and its result
// settles with the value the conditional picks (§27.7.5.2), one job
// later than the synchronous code after the call.
async function pick() {
  return arguments.length > 1 ? arguments[1] : 0;
}
async function count() {
  return arguments.length === 0 ? "none" : arguments.length;
}
pick(1, "two").then((v) => console.log("pick", v));
pick(1).then((v) => console.log("pick one", v));
count().then((v) => console.log("count", v));
count(1, 2, 3).then((v) => console.log("count three", v));
console.log("sync end");
