// for-await GetIterator with hint=async (§7.4.2 step 2.a): a user
// `@@asyncIterator` outranks everything, and a rejected element
// Promise forwards as a catchable throw that closes the loop.
const src: any = {
  [Symbol.asyncIterator]() {
    let i = 0;
    return {
      next() {
        i++;
        return Promise.resolve({ value: i * 100, done: i > 3 });
      },
    };
  },
};
const rejecting: any = [
  Promise.resolve(1),
  Promise.reject(new Error("boom")),
  Promise.resolve(3),
];
async function main() {
  for await (const v of src) console.log(v);
  try {
    for await (const v of rejecting) console.log(v);
  } catch (e: any) {
    console.log("caught", e.message);
  }
}
main();
