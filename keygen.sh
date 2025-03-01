#!/bin/bash

# This script is used for RSA key generation in specified directory

# Check if the directory is specified
if [ -z "$1" ]; then
    echo "Usage: $0 <directory>"
    exit 1
fi

# Check if the directory exists
if [ ! -d "$1" ]; then
    echo "Directory $1 does not exist"
    exit 1
fi

# Generate RSA key pair
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$1/private.pem"
openssl pkcs8 -topk8 -inform PEM -outform PEM -nocrypt -in "$1/private.pem" -out "$1/private.pkcs8.pem"

echo "Generated two files in $1 directory:"
echo "  private.pem - PKCS#1 format"
echo "  private.pkcs8.pem - PKCS#8 format. USE THIS ONE!"
