// `await` of an object at module top level settles through
// PromiseResolve: a callable `then` runs, anything else answers itself
const thenable = {
  then(resolve: any, reject: any) {
    resolve(42);
  },
};
const v = await thenable;
console.log(v, v === 42);

const plain = { a: 1 };
const p = await plain;
console.log(p.a, p === plain);

const rejecting = {
  then(_resolve: any, reject: any) {
    reject(new Error("no"));
  },
};
try {
  await rejecting;
  console.log("unreached");
} catch (e: any) {
  console.log("caught", e.message);
}

const throwing = {
  then() {
    throw new Error("boom");
  },
};
try {
  await throwing;
  console.log("unreached");
} catch (e: any) {
  console.log("caught", e.message);
}

class Later {
  then(resolve: any) {
    resolve("from class");
  }
}
console.log(await new Later());

const chained = {
  then(resolve: any) {
    resolve({
      then(inner: any) {
        inner(7);
      },
    });
  },
};
console.log(await chained);

const notCallable = { then: 5 };
const n = await notCallable;
console.log(n.then, n === notCallable);

const arr = [1, 2, 3];
const a = await arr;
console.log(a.length, a === arr);
