// rotation 240 — compound / logical assignment through a private
// accessor pair with an untyped setter param (the t262
// left-hand-side-private-reference-accessor-property family crashed
// exit 139 on the same arg_conv bypass the public p25g form hit).
class C {
  #v;
  get #p() {
    return this.#v;
  }
  set #p(val) {
    this.#v = val;
  }
  run() {
    this.#v = 10;
    this.#p -= 3;
    return this.#v;
  }
  runOr() {
    this.#v = null;
    this.#p ||= 7;
    return this.#v;
  }
  runNullish() {
    this.#v = undefined;
    this.#p ??= "filled";
    return this.#v;
  }
  runMul() {
    this.#v = 6;
    this.#p *= 7;
    return this.#v;
  }
}
const c = new C();
console.log(c.run());
console.log(c.runOr());
console.log(c.runNullish());
console.log(c.runMul());
