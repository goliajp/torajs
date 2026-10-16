// Arguments `.call` / `.apply` / a spread hand a function value
// without listing them still reach its parameters' width: a fraction
// that arrives that way is not cut to an integer.
const a = [2.5];
const g = function (this: any, x: number) {
  return x;
};
console.log(g.apply({}, a), g.call({}, ...a));
const g1 = function (this: any, x: number) {
  return x * 2;
};
console.log(g1.call({}, 1), g1.apply({}, a));
const g2 = function (this: any, x: number, y: number) {
  return x + y;
};
console.log(g2.call({}, 1, 2), g2.call({}, 1, ...a));
function f(this: any, x: number) {
  return x;
}
console.log(f.apply({}, a), f.call({}, ...a), f.apply({}, [1.5]), f.call({}, 0.5));
function d2(this: any, x: number, y: number) {
  return x + y;
}
console.log(d2.call({}, 1, 2), d2.call({}, 1, ...a), d2.apply({}, [1, ...a]), d2.apply({}, [0.25, 3]));
const xs = [1.5, 2.5];
const k2 = function (x: number, y: number) {
  return x + y;
};
console.log(k2(1, 2), k2.call(null, ...xs), k2.apply(null, xs));
const k3 = function (x: number, y: number) {
  return x + y;
};
console.log(k3(1, 2), k3(...xs), k3(1, ...[0.5]));
function inner() {
  const ys = [0.5, 0.25];
  const h1 = function (this: any, x: number) {
    return x * 2;
  };
  const h2 = function (this: any, x: number, y: number) {
    return x + y;
  };
  const h3 = function (x: number, y: number) {
    return x + y;
  };
  const h4 = function (x: number, y: number) {
    return x + y;
  };
  const h5 = (x: number, y: number) => x * y;
  return [
    h1.call({}, 3),
    h1.apply({}, ys),
    h2.call({}, 1, 2),
    h2.call({}, 1, ...ys),
    h2.apply({}, ys),
    h3(1, 2),
    h3(...ys),
    h4(1, 2),
    h4.call(null, ...ys),
    h4.apply(null, ys),
    h5(1, 2),
    h5(...ys),
  ].join(",");
}
console.log(inner());
