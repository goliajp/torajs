package main

import "fmt"

func gcd(a, b uint64) uint64 {
	for b != 0 {
		t := b
		b = a % b
		a = t
	}
	return a
}

func main() {
	var total uint64 = 0
	const target uint64 = 1234567
	for i := uint64(1); i <= 1000000; i++ {
		total += gcd(i, target)
	}
	fmt.Println(total)
}
