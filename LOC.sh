cd /workspace/ai_sandbox/canon

total=0
git log --reverse --numstat --pretty=format:"commit %H" \
| awk '
/^commit/ { c=$2 }
/\.rs$/ {
  add=$1; del=$2
  if (add != "-" && del != "-") {
    total+=add-del
  }
}
!NF {
  print c, total
}
'
