package main

import (
	"fmt"
	"os"
	"strconv"
)

func survivor(people uint64, k uint64) uint64 {
	var r uint64 = 0
	for i := uint64(2); i <= people; i++ {
		r = (r + k) % i
	}
	return r
}

func main() {
	limit, _ := strconv.ParseUint(os.Args[1], 10, 64)
	people, _ := strconv.ParseUint(os.Args[2], 10, 64)
	var total uint64 = 0
	for k := uint64(1); k <= limit; k++ {
		total += survivor(people, k)
	}
	fmt.Println(total)
}
