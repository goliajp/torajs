const wr: any = Promise.withResolvers();
wr.promise.catch((e: any) => {
  console.log("caught:", e);
});
wr.reject("boom");
console.log("sync end");
