// `"__pro" + "to__"` as a computed key reaches the `__proto__`
// accessor, whose setter gives the object %Promise.prototype%.

type Plain = { a: number };

const box: Plain = { a: 5 };
const d = Object.getOwnPropertyDescriptor(Object.prototype, "__pro" + "to__");
d!.set!.call(box, Promise.prototype);
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
