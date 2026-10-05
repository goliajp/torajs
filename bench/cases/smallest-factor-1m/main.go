package main

import (
	"fmt"
	"os"
	"strconv"
)

func smallestFactor(n uint64) uint64 {
	for d := uint64(2); d*d <= n; d++ {
		if n%d == 0 {
			return d
		}
	}
	return n
}

func main() {
	limit, _ := strconv.ParseUint(os.Args[1], 10, 64)
	var total uint64 = 0
	for n := uint64(2); n < limit; n++ {
		total += smallestFactor(n)
	}
	fmt.Println(total)
}
