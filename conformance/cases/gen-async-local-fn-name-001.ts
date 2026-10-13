// a function value bound by let / const inside a generator or async body keeps the binder's name
function* gen() {
  let h = function () {
    return 1;
  };
  const a = () => 2;
  var v = function () {
    return 3;
  };
  yield h.name;
  yield a.name;
  yield v.name;
  const named = function inner() {
    return 4;
  };
  yield named.name;
  const notAnon = (0, function () {
    return 5;
  });
  yield notAnon.name;
}
console.log([...gen()].join("|"));
async function as1() {
  let h = function () {
    return 1;
  };
  const a = async () => 2;
  await 0;
  return h.name + "," + a.name;
}
as1().then((v) => console.log("async", v));
function plain() {
  let h = function () {
    return 1;
  };
  const k = () => 0;
  return h.name + "," + k.name;
}
console.log(plain());
