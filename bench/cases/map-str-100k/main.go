package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	m := make(map[string]int64)
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	modulus, _ := strconv.ParseInt(os.Args[2], 10, 64)
	for i := int64(0); i < n; i++ {
		j := i % modulus
		var key string
		if i%2 == 0 {
			key = "key" + strconv.FormatInt(j, 10)
		} else {
			key = "ключ" + strconv.FormatInt(j, 10)
		}
		m[key] += i
	}
	var total int64 = 0
	for _, v := range m {
		total += v
	}
	fmt.Println(len(m), total)
}
