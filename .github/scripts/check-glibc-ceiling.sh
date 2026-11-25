#!/usr/bin/env bash
# Fails if a Linux binary needs a newer glibc than the oldest one Trail supports.
#
# Usage: check-glibc-ceiling.sh <binary> [ceiling]
#
# A binary linked on a new distribution asks for that distribution's glibc. When
# the release was built natively on ubuntu-latest, v1.14.1 asked for 2.39, which
# Red Hat 9 and its rebuilds (glibc 2.34), and most shared hosting, do not have.
# The loader only warns about a "weak" version, so the binary still started there,
# but nothing promises the next missing symbol will be weak. The release now
# builds Linux inside the cross container, whose old glibc keeps the requirement
# low; this check is what notices if that ever stops being true.
#
# The ceiling, 2.28, is Red Hat 8 and Debian 10. readelf reads any ELF file, so
# this works for the aarch64 binary on an x86_64 runner too.
set -euo pipefail

bin="$1"
ceiling="${2:-2.28}"

needed="$(readelf -V --wide "$bin" \
  | grep -oE 'Name: GLIBC_[0-9]+\.[0-9]+' \
  | sed 's/^Name: GLIBC_//' \
  | sort -V | tail -n 1)"

if [[ -z "$needed" ]]; then
  echo "::error::found no glibc version requirements in $bin"
  exit 1
fi

echo "$bin needs glibc $needed; the ceiling is $ceiling"
highest="$(printf '%s\n%s\n' "$needed" "$ceiling" | sort -V | tail -n 1)"
if [[ "$highest" != "$ceiling" ]]; then
  echo "::error::$bin needs glibc $needed, newer than the supported $ceiling"
  exit 1
fi
