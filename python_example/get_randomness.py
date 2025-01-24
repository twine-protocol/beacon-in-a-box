# just outputs 512 bits of system randomness

import os
import sys

def main():
    # get randomness
    randomness = os.urandom(64)
    # output randomness
    sys.stdout.buffer.write(randomness)
    sys.stdout.buffer.flush()

if __name__ == "__main__":
    main()