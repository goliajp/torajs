// RFC 20260916-private-element-brand slice 2c — private methods and
// accessors of a class that adopted a Proxy or a plain object resolve on
// that class's prototype, with the adopted object as `this`.

class Base { constructor(o: any) { return o; } }
const log: string[] = [];
const handler = { get(t: any, k: any) { log.push(String(k)); return t[k]; } };
class ProxyBase { constructor() { return new Proxy(this, handler); } }
class G extends ProxyBase {
  get #f() { return 3; }
  #m(x: number) { return x * 2; }
  method() { return this.#f + this.#m(5); }
}
const t: any = new G();
console.log(t.method(), JSON.stringify(log));
class H extends Base {
  #v = 1;
  get #g() { return this.#v + 10; }
  #inc() { this.#v++; return this.#v; }
  static run(o: any) { return [o.#g, o.#inc(), o.#g].join(); }
}
const plain: any = {};
new H(plain);
console.log(H.run(plain), Object.keys(plain).length);
try { H.run({}); } catch (e) { console.log(e instanceof TypeError); }
