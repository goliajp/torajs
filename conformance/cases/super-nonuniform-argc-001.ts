// Two subclasses calling `super` with different counts. The static
// argv face abstains when the counts disagree (it can only fold one
// number), which used to drop the parent back to its declared arity;
// the runtime channel carries each site's own count (rotation 616).
class P {
  constructor() {
    console.log("len", arguments.length, "first", arguments[0], "last", arguments[arguments.length - 1]);
  }
}

class One extends P {
  constructor() { super(1) }
}
class Three extends P {
  constructor() { super(1, 2, 3) }
}
class Spread extends P {
  constructor() { const xs = [7, 8]; super(...xs) }
}
class Mixed extends P {
  constructor() { const xs = [7, 8]; super(0, ...xs) }
}
class None extends P {
  constructor() { super() }
}

new One();
new Three();
new Spread();
new Mixed();
new None();
