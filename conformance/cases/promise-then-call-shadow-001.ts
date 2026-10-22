// A call of a function whose return type is written `Promise<T>` seeds
// the next handler with T only when the name is bound once: a local
// `const get` inside `run` shadows the top-level `get`, and its promise
// settles a string; so does the parameter `get` of `viaParam`.

function get(): Promise<number> {
  return Promise.resolve(1);
}
function run() {
  const get = () => Promise.resolve("s");
  get().then((v) => console.log("inner", typeof v, v.length));
}
function viaParam(get: () => Promise<string>) {
  get().then((v) => console.log("param", typeof v, v.length));
}
run();
viaParam(() => Promise.resolve("ab"));
get().then((v) => console.log("outer", typeof v, v + 1));
