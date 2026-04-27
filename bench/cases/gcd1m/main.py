def gcd(a, b):
    while b != 0:
        t = b
        b = a % b
        a = t
    return a


total = 0
target = 1234567
for i in range(1, 1000001):
    total += gcd(i, target)

print(total)
