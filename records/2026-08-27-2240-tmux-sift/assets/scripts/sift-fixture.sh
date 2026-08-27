#!/usr/bin/env bash
# Corpus for verify-sift-jump.sh. It lives in a file rather than being typed at
# the prompt on purpose: a long typed command is echoed into the scrollback and
# wraps, so the fixture text would appear twice — once as output and once inside
# the echoed command line — and index assertions would match the wrong copy.
for i in $(seq 0 199); do printf 'row%03d aa%03d bb%03d cc%03d\n' "$i" "$i" "$i" "$i"; done
printf '中文測試 aa999 尾巴\n'
