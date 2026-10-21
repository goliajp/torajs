// a RegExp in a class field reaches the any world with its instance
class Holder {
  re: RegExp
  constructor() {
    this.re = /h(i)/
  }
}
const h: any = new Holder()
console.log(h.re.exec('hi')[1], h.re.global)
console.log(Object.keys(h).join(','))
const t = new Holder()
console.log(t.re.test('hi'))
