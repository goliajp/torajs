const wr: any = Promise.withResolvers();
console.log(typeof wr.promise, typeof wr.resolve, typeof wr.reject);
wr.promise.then((v: any) => {
  console.log("value:", v);
});
wr.resolve(42);
wr.resolve(99);
