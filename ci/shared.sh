#!/usr/bin/env bash

ci_dir=$(dirname "${BASH_SOURCE[0]}")
ci_dir=$(realpath "${ci_dir}")

function retry {
  local tries="${TRIES-5}"
  local timeout="${TIMEOUT-1}"
  local try=0
  local exit_code=0

  while (( try < tries )); do
    if "${@}"; then
      return 0
    else
      exit_code=$?
    fi

    sleep "${timeout}"
    echo "Retrying ..." 1>&2
    try=$(( try + 1 ))
    timeout=$(( timeout * 2 ))
  done

  return ${exit_code}
}

function mkcargotemp {
  local td=
  mkdir -p "$ci_dir"/../target/tmp
  td=$(command mktemp --tmpdir="$ci_dir"/../target/tmp "${@}")
  echo '# Cargo.toml
  [workspace]
  members = ["'"$(basename "$td")"'"]
   ' > "$ci_dir"/../target/tmp/Cargo.toml
  echo "$td"
}
