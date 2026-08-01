// §27.2.4.7 step 2 through the any lane — Promise.resolve(x) where
// x is an any-typed value that IS a promise adopts the cell instead
// of double-wrapping it (`Promise.resolve(p) === p`): the then
// callback sees the inner value, not a Promise object.

function mk(): any {
  return Promise.resolve("adopted");
}
Promise.resolve(mk()).then(function (v) {
  console.log("adopt", v);
});

// non-promise any values still mint a fulfilled promise
function num(): any {
  return 7;
}
Promise.resolve(num()).then(function (v) {
  console.log("num", v);
});

// rejected inner promise passes through with its rejection
function bad(): any {
  return Promise.reject(new Error("nope"));
}
Promise.resolve(bad()).catch(function (e: any) {
  console.log("rejected", e.message);
});

console.log("main-end");
