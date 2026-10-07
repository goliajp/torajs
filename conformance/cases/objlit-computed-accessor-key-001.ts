// computed accessor keys in an object literal read a const binding
const K = "kk";
const S = "ss";
function mk() {
  let store = 1;
  return {
    get [K]() { return 5; },
    get [S]() { return store; },
    set [S](v: number) { store = v * 3; },
  };
}
const o = mk();
console.log(o.kk);
o.ss = 2;
console.log(o.ss);
