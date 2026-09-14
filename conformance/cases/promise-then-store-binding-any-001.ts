// A bare `let` bound to a certain promise infers `Promise<T>`, whose
// typed surface has no room for an own `then`: the store was the
// checker's "field assignment target must be a struct" reject, and a
// `this`-reading function expression stored there died as an
// unclaimed receiver. The same program with `: any` spelled answers
// bun line for line, so a binding whose `then` the program stores
// widens to `any` (rotation 651; test262 `Promise/any/invoke-then`,
// `await-monkey-patched-promise`).
let promise = Promise.resolve();
let boundThen = promise.then.bind(promise);
let cnt = 0;
promise.then = function (r, j) {
  cnt++;
  console.log("own then", this === promise, typeof r, r.length, typeof j, j.length);
  return boundThen(r, j);
};
Promise.any([promise]).then((_: any) => console.log("any", cnt));

const q = Promise.resolve(3);
Object.defineProperty(q, "then", { value: function (r: any, j: any) { cnt++; r(9); } });
Promise.all([q]).then((v: any) => console.log("all", JSON.stringify(v), cnt));

var p1 = new Promise(function () {});
var p2 = Promise.resolve(2);
p1.then = p2.then = function (a: any, b: any) { cnt++; console.log("chained", this === p1, this === p2); };
Promise.race([p1, p2]);

// No `then` store anywhere: the binding keeps its typed lane.
const plain = Promise.resolve(5);
plain.then((v) => console.log("plain", v + 1));
