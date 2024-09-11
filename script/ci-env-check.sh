#!/bin/bash

echo "=== CI Environment Diagnostics ==="

echo "Checking for wget:"
if command -v wget &> /dev/null; then
    echo "wget is available. Version:"
    wget --version | head -n 1
else
    echo "wget is not available."
fi

echo -e "\nChecking for package managers:"
for pkg_manager in apt-get yum apk dnf zypper; do
    if command -v $pkg_manager &> /dev/null; then
        echo "$pkg_manager is available."
    else
        echo "$pkg_manager is not available."
    fi
done

echo -e "\nSystem information:"
if [ -f /etc/os-release ]; then
    cat /etc/os-release
elif [ -f /etc/lsb-release ]; then
    cat /etc/lsb-release
else
    echo "Unable to determine OS information."
fi

echo -e "\nPATH environment variable:"
echo $PATH

echo -e "\nContents of /usr/bin (first 10 entries):"
ls /usr/bin | head -n 10

echo -e "\nCI-related environment variables:"
env | grep -i "ci" || echo "No CI-specific environment variables found."

echo -e "\n=== End of Diagnostics ==="
