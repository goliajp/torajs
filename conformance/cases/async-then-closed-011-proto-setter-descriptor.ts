// The `__proto__` setter of %Object.prototype% (ECMA-262 §B.2.2.1.2),
// taken out of its descriptor, gives the object %Promise.prototype%:
// an await of it calls that `then` with a receiver that is no promise,
// a TypeError (§27.2.5.4 step 2).

type Plain = { a: number };

const box: Plain = { a: 5 };
const desc = Object.getOwnPropertyDescriptor(Object.prototype, "__proto__");
desc!.set!.call(box, Promise.prototype);
console.log(Object.getPrototypeOf(box) === Promise.prototype);

async function main() {
  try {
    const v = await box;
    console.log("fulfilled", typeof v);
  } catch (e) {
    console.log("rejected", e instanceof TypeError);
  }
}

main();
