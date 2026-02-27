#!/usr/bin/env bash

FILE="AGENT_STATE.md"
TMPDIR="/tmp/agent_state_sections"
mkdir -p "$TMPDIR"
rm -f "$TMPDIR"/*

# Get commits oldest -> newest
git log -n 5 --reverse --format="%h" -- "$FILE" |
while read hash; do
  content=$(git show "$hash:$FILE")

  # Extract each ### N) section
  echo "$content" | awk '
  /^### [0-9]+\)/ {
      if (section_name != "") {
          print section > (tmpdir "/" section_name "." hash)
      }
      section_name=$0
      gsub(/[^0-9]/,"",section_name)   # numeric key
      section=""
      next
  }

  /^### [0-9]+\)/==0 && section_name != "" {
      section=section $0 "\n"
  }

  END {
      if (section_name != "") {
          print section > (tmpdir "/" section_name "." hash)
      }
  }
  ' tmpdir="$TMPDIR" hash="$hash"
done

echo
echo "========== GROUPED FORWARD DELTAS =========="
echo

# For each section number found
for sec in $(ls "$TMPDIR" | sed 's/\..*//' | sort -u); do
  echo
  echo "########################################"
  echo "### SECTION $sec"
  echo "########################################"

  prev=""
  for file in $(ls "$TMPDIR"/$sec.* 2>/dev/null | sort -t. -k2); do
    hash=$(basename "$file" | cut -d. -f2)

    if [ -n "$prev" ]; then
      echo
      echo "---- Δ at $hash ----"
      diff -u "$prev" "$file"
    fi

    prev="$file"
  done
done
