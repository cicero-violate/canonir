# Read phases across all ticks to see the pattern
python3 - << 'EOF'
import os, json, glob

ticks = []
for d in sorted(glob.glob("tick_*"), key=lambda x: int(x.split("_")[1])):
    n = int(d.split("_")[1])
    info = {"tick": n}
    
    # phase
    p = os.path.join(d, "phase.txt")
    if os.path.exists(p):
        info["phase"] = open(p).read().strip()
    
    # has act_error
    info["act_error"] = os.path.exists(os.path.join(d, "act_error.txt"))
    info["act_retry_ok"] = os.path.exists(os.path.join(d, "act_retry_ok.txt"))
    info["has_bash"] = os.path.exists(os.path.join(d, "bash_output.txt"))
    info["has_retry"] = os.path.exists(os.path.join(d, "retry_response.json"))
    
    # read last few lines of bash_output if exists
    bo = os.path.join(d, "bash_output.txt")
    if os.path.exists(bo):
        lines = open(bo).readlines()
        info["bash_tail"] = "".join(lines[-5:]).strip()
    
    ticks.append(info)

# Show phase distribution
from collections import Counter
phases = Counter(t.get("phase","?") for t in ticks)
print("=== PHASE DISTRIBUTION ===")
for ph, cnt in phases.most_common():
    print(f"  {ph}: {cnt}")

print("\n=== ERROR RATE BY TICK RANGE ===")
ranges = [(0,50),(50,100),(100,150),(150,200),(200,250),(250,300),(300,350),(350,411)]
for lo, hi in ranges:
    subset = [t for t in ticks if lo <= t["tick"] < hi]
    errs = sum(1 for t in subset if t["act_error"])
    print(f"  tick {lo:3d}-{hi:3d}: {errs}/{len(subset)} act_errors")

print("\n=== LAST 20 TICKS ===")
for t in ticks[-20:]:
    print(f"  tick_{t['tick']:03d}: phase={t.get('phase','?')} act_error={t['act_error']} retry={t['has_retry']} bash={t['has_bash']}")
    if "bash_tail" in t and t["act_error"]:
        print(f"    bash_tail: {t['bash_tail'][:200]}")
EOF

# Now read the actual response content for recent failing ticks
python3 - << 'EOF'
import os, json, glob

# Focus on last 15 ticks - read response/retry_response content
for d in sorted(glob.glob("tick_3[6-9]*") + glob.glob("tick_4[0-1][0-9]"), key=lambda x: int(x.split("_")[1])):
    n = int(d.split("_")[1])
    print(f"\n{'='*60}")
    print(f"TICK {n}")
    
    for fname in ["phase.txt", "act_error.txt", "bash_output.txt"]:
        fp = os.path.join(d, fname)
        if os.path.exists(fp):
            content = open(fp).read().strip()
            print(f"--- {fname} ---")
            print(content[:500])
    
    # read response json - extract the assistant message/action
    for rname in ["response.json", "retry_response.json"]:
        rp = os.path.join(d, rname)
        if os.path.exists(rp):
            try:
                data = json.load(open(rp))
                print(f"--- {rname} (keys: {list(data.keys())}) ---")
                # Try to get the content/action
                content = json.dumps(data)[:800]
                print(content)
            except Exception as e:
                print(f"  ERROR reading {rname}: {e}")
EOF

# Also check what the agent was doing in the prompt of last ticks
python3 - << 'EOF'
import os, json, glob

for d in sorted(glob.glob("tick_40[5-9]") + glob.glob("tick_41*"), key=lambda x: int(x.split("_")[1])):
    n = int(d.split("_")[1])
    print(f"\n{'='*60} TICK {n}")
    
    fp = os.path.join(d, "prompt.txt")
    if os.path.exists(fp):
        content = open(fp).read().strip()
        print(f"--- prompt.txt (last 800 chars) ---")
        print(content[-800:])
    
    rp = os.path.join(d, "retry_prompt.txt")
    if os.path.exists(rp):
        content = open(rp).read().strip()
        print(f"--- retry_prompt.txt (last 500 chars) ---")
        print(content[-500:])
EOF
