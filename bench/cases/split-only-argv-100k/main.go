package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	input := os.Args[2]
	sep := os.Args[3]
	for i := int64(0); i < n; i++ {
		parts := strings.Split(input, sep)
		total += int64(len(parts))
	}
	fmt.Println(total)
}
