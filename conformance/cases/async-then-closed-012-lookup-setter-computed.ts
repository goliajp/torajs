// `__lookupSetter__` read under a computed key hands out the
// `__proto__` setter (ECMA-262 §B.2.2.5, §B.2.2.1.2), which gives the
// object %Promise.prototype%.

type Plain = { a: number };

const box: Plain = { a: 5 };
const d: any = {};
const k1 = ["__lookup", "Setter__"].join("");
const setter = d[k1](["__pro", "to__"].join(""));
setter.call(box, Promise.prototype);
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
