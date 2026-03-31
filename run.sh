while true;
do   cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start planner;
     echo "Exited with code $?";
     sleep 1;
done
