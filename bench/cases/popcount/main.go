package main

import (
	"fmt"
	"os"
	"strconv"
)

func popcount(x uint64) uint64 {
	n := x
	count := uint64(0)
	for n != 0 {
		n = n & (n - 1)
		count++
	}
	return count
}

func main() {
	limit, _ := strconv.ParseUint(os.Args[1], 10, 64)
	var total uint64 = 0
	for i := uint64(0); i < limit; i++ {
		total += popcount(i)
	}
	fmt.Println(total)
}
