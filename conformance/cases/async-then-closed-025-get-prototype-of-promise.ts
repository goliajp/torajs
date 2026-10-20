// `Object.getPrototypeOf` of a promise hands out %Promise.prototype%,
// an ordinary object whose `then` throws on a receiver that is not a
// promise (ECMA-262 §27.2.5.4 step 2): awaiting it under an object type
// rejects with a TypeError, whether it got there by an `as`, through a
// generic identity or as an array element.

type Plain = { a: number };

function id<T>(x: T): T {
  return x;
}

const byCast = Object.getPrototypeOf(Promise.resolve(1)) as Plain;
const byId = id<Plain>(Object.getPrototypeOf(Promise.resolve(1)));
const inArr: Plain[] = [Object.getPrototypeOf(Promise.resolve(1))];

async function settle(pp: Plain) {
  const v = await pp;
  return v.a;
}

function probe(name: string, pp: Plain): Promise<void> {
  return settle(pp).then(
    () => console.log(name, "ok"),
    (e) => console.log(name, e instanceof TypeError ? "TypeError" : "other"),
  );
}

probe("cast", byCast)
  .then(() => probe("id", byId))
  .then(() => probe("arr", inArr[0]));
