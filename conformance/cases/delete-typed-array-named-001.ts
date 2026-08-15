const arr = [1, 2, 3];
try {
  // @ts-ignore
  console.log(delete arr.length);
} catch (e: any) {
  console.log("throw", e.constructor.name);
}
// @ts-ignore
console.log(delete arr.noSuch, arr.length);
