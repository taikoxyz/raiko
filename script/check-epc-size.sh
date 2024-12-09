#!/usr/bin/env bash

# Run dmesg and filter for EPC section
epc_line=$(sudo dmesg | fgrep EPC)

# Extract the start and end addresses using regex
if [[ $epc_line =~ 0x([0-9a-fA-F]+)-0x([0-9a-fA-F]+) ]]; then
    start_address=0x${BASH_REMATCH[1]}
    end_address=0x${BASH_REMATCH[2]}

    # Calculate the EPC size in GB using Python
    epc_size_gb=$(python3 -c "print(($end_address - $start_address) / 1024 ** 3)")

    echo "EPC Size: $epc_size_gb GB"
else
    echo "EPC section not found in dmesg output."
fi
