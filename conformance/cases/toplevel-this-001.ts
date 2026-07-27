// Cluster #5 (test262) — TOP-LEVEL `this` materializes as the
// module-this `{}` dynobj (bun's CommonJS-flavored exports object):
// member reads / writes, identity, and lexical inheritance into a
// true arrow all ride it; a plain fn keeps its own `this` route.
console.log(this)
console.log(typeof this)
console.log(this.Object)
console.log(this === this)
this.x = 5
console.log(this.x)
const arrow = () => this
console.log(arrow() === this)
var thisobj = this.Function
console.log(thisobj)
var g = this
console.log(g === this)
