Promise.all([Promise.resolve(1), 2, "three"]).then((vs: any) => {
  console.log(vs);
  console.log(vs.length, vs[0] + vs[1]);
});
