// a generic hierarchy's overridden method answers the count its call passed
class A<T> { m(x: T) { return arguments.length } }
class B<T> extends A<T> { m(x: T) { return 10 + arguments.length } }
const a: A<number> = new B<number>()
console.log(a.m(1))
const an: any = a
console.log(an.m(), an.m(1, 2))
