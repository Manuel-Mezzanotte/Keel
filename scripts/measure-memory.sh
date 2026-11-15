#!/bin/bash
# Prints the memory (RSS) of a process and of everything it started, grouped
# by process name, so Keel and other editors are measured the same way.
#
# Usage: scripts/measure-memory.sh PID
set -euo pipefail

root="${1:?usage: $0 PID}"

ps -axo pid=,ppid=,rss=,comm= | awk -v root="$root" '
{
  pid = $1; ppid = $2; rss = $3
  $1 = ""; $2 = ""; $3 = ""
  name = substr($0, 4)
  sub(/^.*\//, "", name)
  parent[pid] = ppid; memory[pid] = rss; label[pid] = name
}
END {
  for (pid in parent) {
    current = pid
    for (depth = 0; depth < 64 && current != "" && current != 0; depth++) {
      if (current == root) { inside[pid] = 1; break }
      current = parent[current]
    }
  }
  for (pid in inside) {
    total += memory[pid]; count[label[pid]]++; sum[label[pid]] += memory[pid]
  }
  for (name in sum) printf "%9.1f MB  %3d  %s\n", sum[name] / 1024, count[name], name | "sort -rn"
  close("sort -rn")
  printf "%9.1f MB  total, %d processes\n", total / 1024, length(inside)
}'
