// §13.3.10 step 7 — an abrupt completion from ToString(specifier)
// rejects the promise with the thrown value.
const obj: any = {
  toString() {
    throw "custom error";
  },
};
import(obj).catch((e: any) => {
  console.log("abrupt", e);
});
