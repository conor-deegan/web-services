#!/bin/zsh

# Initialize variables
total_time=0
N=100

# Loop 1000 times
for i in {1..$N}; do
    # Capture start time in milliseconds using Python
    start_time=$(python3 -c "import time; print(int(time.time() * 1000))")
    
    # Run the command
    cargo run -- -X GET http://example.com/api/spells > /dev/null 2>&1

    # Capture end time in milliseconds using Python
    end_time=$(python3 -c "import time; print(int(time.time() * 1000))")

    # Calculate the response time in milliseconds
    response_time=$(( end_time - start_time ))
    total_time=$(( total_time + response_time ))
done

# Calculate mean response time
mean_time=$(( total_time / N ))

echo "Mean response time: ${mean_time}ms"