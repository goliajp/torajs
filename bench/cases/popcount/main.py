import sys

def popcount(x):
    n = x
    count = 0
    while n != 0:
        n = n & (n - 1)
        count += 1
    return count


total = 0
for i in range(int(sys.argv[1])):
    total += popcount(i)

print(total)
