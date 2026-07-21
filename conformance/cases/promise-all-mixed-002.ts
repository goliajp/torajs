Promise.all([1, Promise.reject("bad"), 3]).catch((e: any) => {
  console.log("err", e);
});
