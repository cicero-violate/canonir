nvidia-smi dmon -s u -d 1 &
SMON=$!
sleep 1
cargo run --example gpu_example --features cuda -- --stress
kill $SMON
