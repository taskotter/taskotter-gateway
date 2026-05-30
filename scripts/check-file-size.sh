#!/usr/bin/env sh
set -eu

limit="${FILE_SIZE_LIMIT_LINES:-320}"

too_large=$(find src docs scripts -type f \
  ! -path '*/target/*' \
  ! -name 'Cargo.lock' \
  -exec wc -l {} + \
  | awk -v limit="$limit" '$1 > limit && $2 != "total" { print $2 " has " $1 " lines; limit is " limit }')

if [ -n "$too_large" ]; then
  echo "$too_large" >&2
  exit 1
fi
