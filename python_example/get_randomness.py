import os
import sys
import hashlib

def main():
    # get randomness from 3 sources
    randomness1 = os.urandom(64)
    randomness2 = os.urandom(64)  # additional source
    randomness3 = os.urandom(64)  # additional source

    # combine the randomness
    combined_randomness = randomness1 + randomness2 + randomness3

    # hash the combined randomness to ensure it's consistent in size
    hashed_randomness = hashlib.sha3_512(combined_randomness).digest()
    # output hashed randomness
    sys.stdout.buffer.write(hashed_randomness)
    sys.stdout.buffer.flush()

if __name__ == "__main__":
    main()