package main

import (
	"fmt"
	"os"
	"strconv"
)

func ackermann(m, n uint64) uint64 {
	if m == 0 {
		return n + 1
	}
	if n == 0 {
		return ackermann(m-1, 1)
	}
	return ackermann(m-1, ackermann(m, n-1))
}

func main() {
	m, _ := strconv.ParseUint(os.Args[1], 10, 64)
	n, _ := strconv.ParseUint(os.Args[2], 10, 64)
	fmt.Println(ackermann(m, n))
}
