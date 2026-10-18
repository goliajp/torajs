package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	for i := int64(0); i < n; i++ {
		s := strconv.FormatInt(i, 10)
		total = total + int64(len(s))
	}
	fmt.Println(total)
}
