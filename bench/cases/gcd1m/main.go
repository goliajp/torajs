package main

import (
	"fmt"
	"os"
	"strconv"
)

func gcd(a, b uint64) uint64 {
	for b != 0 {
		t := b
		b = a % b
		a = t
	}
	return a
}

func main() {
	limit, _ := strconv.ParseUint(os.Args[1], 10, 64)
	var total uint64 = 0
	target, _ := strconv.ParseUint(os.Args[2], 10, 64)
	for i := uint64(1); i <= limit; i++ {
		total += gcd(i, target)
	}
	fmt.Println(total)
}
