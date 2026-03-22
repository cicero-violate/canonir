#!/usr/bin/env bash
set -e
mkdir -p src/generated
> src/generated/mod.rs
for i in $(seq 1 250); do
  echo "pub mod file_$i;" >> src/generated/mod.rs
  echo "// file $i" > src/generated/file_$i.rs
  for j in $(seq 1 200); do
    echo "pub fn f_${i}_${j}() -> usize { $j }" >> src/generated/file_$i.rs
  done
done

