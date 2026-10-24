// An async function with an unannotated parameter is compiled once per
// argument shape; the copies made for a `null` and an `undefined`
// argument carry that parameter as a null pointer across the awaits of
// a frame-suspended body (an await inside try / finally), and read it
// back after resuming.
async function check(func) {
  let caught = false;
  try {
    await Promise.reject(new Error("no"));
  } catch (e) {
    caught = true;
  } finally {
    console.log("checked", caught, typeof func === "function" ? "fn" : "" + func, typeof func);
  }
}
async function main() {
  await check(null);
  await check(undefined);
  await check();
  await check(10);
  await check("s");
  await check({});
  await check(function () { return 1; });
  console.log("done");
}
main();
