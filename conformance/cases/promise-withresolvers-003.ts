const wr: any = Promise.withResolvers();
console.log(wr.resolve.name === "", wr.resolve.length, wr.reject.length);
wr.resolve("early");
wr.promise.then((v: any) => {
  console.log("late attach:", v);
});
