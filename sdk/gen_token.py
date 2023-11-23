import string
import random

# initializing size of string
N = 64

res = ''.join(random.choices(string.ascii_letters +
                             string.digits, k=N))

# print result
print("The generated random string : " + str(res))
