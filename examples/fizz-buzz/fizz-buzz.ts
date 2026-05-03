// FizzBuzz — classic. Prints numbers 1..100 with substitutions.
//
// Exercises: number-to-string conversion, string concatenation,
// integer modulo, conditional logic.

function fizzBuzz(n: number): void {
  for (let i = 1; i <= n; i++) {
    if (i % 15 === 0) {
      console.log("FizzBuzz");
    } else if (i % 3 === 0) {
      console.log("Fizz");
    } else if (i % 5 === 0) {
      console.log("Buzz");
    } else {
      console.log(i.toString());
    }
  }
}

fizzBuzz(20);
