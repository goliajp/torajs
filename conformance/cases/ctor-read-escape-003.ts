// a constructor read stored into an object or returned keeps the
// statics callable where it lands
function ctorOf(x: any): any {
  return x.constructor
}
function stash(o: any) {
  const holder: any = {}
  holder.k = o.constructor
  return holder
}
console.log(stash({ e: 5 }).k.keys({ f: 6 }), ctorOf({}).entries({ g: 7 }))
