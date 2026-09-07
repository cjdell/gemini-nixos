#!/bin/bash
# run-job.sh — start long host operations DETACHED, then poll them to
# completion WITHOUT ever stalling a session.
#
# WHY THIS EXISTS (2026-09-06): ad-hoc one-liners of the form
#   nohup nix develop --command bash -c '…make…' > /tmp/x.log 2>&1 &
#   echo started; for i in $(seq 1 240); do sleep 5; if ! kill -0 \
#   $(pgrep -f 'make ARCH=arm64' | head -1) 2>/dev/null; then break; fi; done
# are fragile in three ways (all seen in this repo):
#   1. `pgrep -f <pattern>` matches the CALLER's own command line — the
#      loop's bash -c contains the literal pattern text — so the "wrong"
#      process (often the loop itself) is what kill -0 tests. The loop then
#      spins the full 240×5 s = 20 min before the tool call returns.
#   2. `pgrep | head -1` can pick an older/stale process with a matching
#      cmdline from an earlier build (kernel tree builds share the string
#      'make ARCH=arm64').
#   3. The loop lives in the same tool call as the launch, so a missed
#      completion = a session stall that only the tool timeout ends.
#
# run-job.sh fixes this by tracking the REAL process (a pidfile written by
# the job itself, guarded against pid reuse via /proc/<pid>/stat starttime)
# and by making every `wait` call bounded (--timeout) and re-runnable:
# a missed completion costs one extra `wait`, never a stall.
#
# The job is launched with `setsid` into its own session, so it survives
# the tool call that started it (no controlling tty, no process-group
# cleanup can reach it).
#
# Usage:
#   bash bin/run-job.sh start NAME [--workdir DIR] -- CMD [ARG…]
#       Launch CMD detached. Returns immediately. State+log:
#         logs/jobs/NAME/{pid,pst,status,cmd,started,log}
#       Refuses if NAME is already running. CMD runs with cwd = DIR
#       (default: repo root). Prefix env vars the usual way
#       (env A=1 B=2 CMD…) or export them.
#   bash bin/run-job.sh wait NAME [--timeout S] [--interval S]
#                                  [--tail N] [--quiet]
#       Polls until the job exits, printing a progress line every 30 s.
#       Returns the MOMENT it finishes (never spins). Exit codes:
#         0  finished rc=0
#         1  finished rc!=0, or died without recording a status
#         2  still running when --timeout hit  → re-run `wait` later
#         3  usage error / no such job / job still running (start/clean)
#   bash bin/run-job.sh status NAME    one line: running|finished|dead
#   bash bin/run-job.sh tail NAME [N]  show the job log (default 40)
#   bash bin/run-job.sh stop NAME [--grace S]
#       TERM the job's process group, escalate to KILL after grace.
#   bash bin/run-job.sh clean NAME     drop the job state (not running)
#   bash bin/run-job.sh list           all known jobs + state
#   bash bin/run-job.sh wait-file LOG END_REGEX [--timeout S]
#                   [--interval S] [--min-idle N] [--tail N]
#       SALVAGE poll for an operation that was started WITHOUT this script
#       (only a log file exists, no pidfile). Returns when END_REGEX
#       matches the log AND the log size has been unchanged for N
#       consecutive checks (default N=2). rc 0 done / 2 timeout.
#
# Canonical kernel-build flow (replaces every inline nohup/pgrep loop):
#   bash bin/run-job.sh start b330 -- env WITH_CONSYS=1 BUILD_NN=330 \
#       nix develop --command bash build/build-gemini-linux-6.6.sh fbcon
#   bash bin/run-job.sh wait b330     # rc 0 → built; rc 2 → wait again
#
# Job-state location: logs/jobs/ (gitignored). Logs are never truncated by
# run-job — `clean NAME` after you have what you need.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE="$ROOT/logs/jobs"
LAUNCHER="$STATE/.launcher.sh"
INTERVAL_DEFAULT=5
TIMEOUT_DEFAULT=1800      # safety valve; completion is detected promptly
PROGRESS_EVERY=30         # seconds between progress lines in `wait`

mkdir -p "$STATE"

# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

die() { echo "run-job.sh: $*" >&2; exit 3; }

usage() {
  echo "usage: bash bin/run-job.sh {start|wait|status|tail|stop|clean|list|wait-file} … (see header comments)" >&2
  exit 3
}

check_name() {
  case "$1" in
    "" | "." | ".." | -* | *[!A-Za-z0-9_.-]*)
      die "invalid job name '$1' (use [A-Za-z0-9_.-], no leading '-')" ;;
  esac
}

ensure_launcher() {
  # Generic argv-driven launcher (one shared copy). Written only once; the
  # job's pidfile holds THIS process's pid (setsid exec'd it, so it is the
  # new session + process-group leader → kill -- -PID reaches the whole job).
  # The marker line lets future edits self-heal stale copies.
  if [ ! -x "$LAUNCHER" ] || ! grep -q 'launcher-version: 2' "$LAUNCHER" 2>/dev/null; then
    cat > "$LAUNCHER" <<'EOF'
#!/usr/bin/env bash
# run-job launcher v2 — argv: PIDFILE STATUS WORKDIR [CMD ARG…]
# launcher-version: 2
PIDFILE=$1; STATUS=$2; WORKDIR=$3; shift 3
echo $$ > "$PIDFILE"                      # first thing: make us trackable
if ! cd "$WORKDIR" 2>/dev/null; then
  echo "launcher: workdir does not exist: $WORKDIR" >&2
  echo 127 > "$STATUS"; exit 127
fi
trap 'echo 143 > "$STATUS"; exit 143' TERM INT HUP
"$@"
rc=$?
echo "$rc" > "$STATUS"
exit "$rc"
EOF
    chmod +x "$LAUNCHER"
  fi
}

pstat() { awk -v f="$2" '{print $f}' "/proc/$1/stat" 2>/dev/null || true; }

# alive PID WANT_STARTTIME — pid exists, starttime matches (not reused),
# and is not a zombie.
alive() {
  local pid="$1" want="$2" got state
  [ -n "$pid" ] || return 1
  [ -r "/proc/$pid/stat" ] || return 1
  got="$(pstat "$pid" 22)"
  state="$(pstat "$pid" 3)"
  [ -n "$got" ] && [ "$got" = "$want" ] && [ "$state" != "Z" ]
}

now() { date +%s; }

jobdir() { printf '%s/%s' "$STATE" "$1"; }

read_state() { # jobdir -> sets pid, want, rc (rc empty if no status)
  pid=""; want=""; rc=""
  [ -f "$1/pid" ] && pid="$(cat "$1/pid" 2>/dev/null | tr -d '[:space:]' || true)"
  [ -f "$1/pst" ] && want="$(cat "$1/pst" 2>/dev/null | tr -d '[:space:]' || true)"
  [ -f "$1/status" ] && rc="$(cat "$1/status" 2>/dev/null | tr -d '[:space:]' || true)"
  return 0
}

parse_wait_opts() { # $1 = "wait"|"wait-file"; consumes $@; sets timeout/interval/tailn/quiet/minidle
  local sub="$1"; shift
  timeout="$TIMEOUT_DEFAULT"; interval="$INTERVAL_DEFAULT"; tailn=5; quiet=0; minidle=2
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --timeout)  [ "$#" -ge 2 ] || die "$sub: --timeout needs SECONDS"; timeout="$2"; shift 2 ;;
      --interval) [ "$#" -ge 2 ] || die "$sub: --interval needs SECONDS"; interval="$2"; shift 2 ;;
      --tail)     [ "$#" -ge 2 ] || die "$sub: --tail needs N"; tailn="$2"; shift 2 ;;
      --quiet)    quiet=1; shift ;;
      --min-idle) [ "$#" -ge 2 ] || die "$sub: --min-idle needs N"; minidle="$2"; shift 2 ;;
      *) die "$sub: unknown option '$1'" ;;
    esac
  done
  case "$timeout" in *[!0-9]* | "") die "$sub: --timeout must be an integer" ;; esac
  case "$interval" in *[!0-9]* | "") die "$sub: --interval must be an integer" ;; esac
}

show_tail() { # logfile tailn
  if [ "$2" -gt 0 ] && [ -s "$1" ]; then
    echo "--- last $2 line(s) of $(basename "$1") ---"
    tail -n "$2" "$1" 2>/dev/null || true
    echo "--------------------------------------------"
  fi
}

# ---------------------------------------------------------------------------
# start
# ---------------------------------------------------------------------------

cmd_start() {
  [ "$#" -ge 3 ] || usage
  local name="$1"; shift
  check_name "$name"
  local wd="$ROOT"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --workdir) [ "$#" -ge 2 ] || die "start: --workdir needs DIR"; wd="$2"; shift 2 ;;
      --) shift; break ;;
      -*) die "start: unknown option '$1' (options must precede --)" ;;
      *) die "start: unexpected argument '$1' (use: start NAME [--workdir DIR] -- CMD…)" ;;
    esac
  done
  [ "$#" -ge 1 ] || die "start: nothing after '--'"
  [ -d "$wd" ] || die "start: workdir does not exist: $wd"
  local CMD=("$@")

  local job; job="$(jobdir "$name")"
  local pid="" want="" rc=""
  read_state "$job"
  if alive "$pid" "$want"; then
    die "job '$name' is still running (pid $pid) — 'wait' it or 'stop' it first"
  fi
  rm -rf "$job"; mkdir -p "$job"
  local log="$job/log" status="$job/status" pidfile="$job/pid"

  # provenance
  {
    printf 'started: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'workdir: %s\n' "$wd"
    printf 'cmd: '
    printf '%q ' "${CMD[@]}"
    printf '\n'
  } > "$job/cmd"
  date +%s > "$job/started"

  ensure_launcher
  # New session (setsid) + full redirection: survives this tool call, and
  # the launcher (exec'd by setsid) is the session/pg leader whose pid the
  # pidfile records.
  ( exec setsid "$LAUNCHER" "$pidfile" "$status" "$wd" "${CMD[@]}" ) >"$log" 2>&1 &

  # Wait (≤5 s) for the launcher to write its pid, then record the /proc
  # starttime used as the pid-reuse guard.
  pid=""
  for _ in $(seq 1 50); do
    [ -f "$pidfile" ] && { pid="$(cat "$pidfile" | tr -d '[:space:]' || true)"; [ -n "$pid" ] && break; }
    sleep 0.1
  done
  if [ -z "$pid" ]; then
    if [ -f "$status" ]; then
      die "job '$name' exited before start returned (rc=$(cat "$status")) — log: $log"
    fi
    die "job '$name' failed to launch — see $log"
  fi
  pstat "$pid" 22 > "$job/pst"

  echo "started job '$name' (pid $pid)"
  echo "  cmd:  $(tr '\n' ' ' < "$job/cmd" | sed 's/^cmd: //')"
  echo "  log:  $log"
  echo "  wait: bash bin/run-job.sh wait $name"

  # Fast feedback for instant failures (e.g. command not found): give the
  # job 2 s to die on its own, then surface it.
  sleep 2
  local want2; want2="$(cat "$job/pst")"
  if ! alive "$pid" "$want2"; then
    if [ -f "$status" ]; then
      echo "note: job '$name' already exited rc=$(cat "$status") — run 'wait' for details"
    fi
  fi
}

# ---------------------------------------------------------------------------
# wait / status / tail / stop / clean / list
# ---------------------------------------------------------------------------

cmd_wait() {
  [ "$#" -ge 1 ] || usage
  local name="$1"; shift
  check_name "$name"
  local timeout interval tailn quiet minidle
  parse_wait_opts wait "$@"

  local job; job="$(jobdir "$name")"
  [ -f "$job/pid" ] || die "no such job '$name' (state dir: $job)"
  local pid="" want="" rc="" t0 deadline elapsed
  read_state "$job"
  t0="$(now)"; deadline=$(( t0 + timeout ))
  local started_at=0
  [ -f "$job/started" ] && started_at="$(cat "$job/started" | tr -d '[:space:]' || true)"

  if ! alive "$pid" "$want"; then
    # already finished before this wait call
    :
  else
    local last_report=0
    while alive "$pid" "$want"; do
      if [ "$(now)" -ge "$deadline" ]; then
        echo "JOB $name STILL RUNNING after ${timeout}s (pid $pid)"
        echo "  → re-run when ready:  bash bin/run-job.sh wait $name [--timeout S]"
        return 2
      fi
      sleep "$interval"
      if [ "$quiet" -eq 0 ]; then
        elapsed=$(( $(now) - started_at ))
        if [ $(( elapsed - last_report )) -ge "$PROGRESS_EVERY" ]; then
          last_report=$elapsed
          local lines lastline
          lines="$(wc -l < "$job/log" 2>/dev/null | tr -d '[:space:]' || echo 0)"
          lastline="$(tail -n 1 "$job/log" 2>/dev/null | cut -c1-140 || true)"
          printf '[%s] running %ds (pid %s, log %s lines) last: %s\n' \
            "$name" "$elapsed" "$pid" "$lines" "$lastline"
        fi
      fi
    done
  fi

  elapsed=$(( $(now) - started_at ))
  [ -f "$job/status" ] && rc="$(cat "$job/status" | tr -d '[:space:]' || true)"
  if [ -z "$rc" ]; then
    echo "JOB $name DIED WITHOUT RECORDING A STATUS (hard kill / host reboot?)"
    [ "$quiet" -eq 0 ] && show_tail "$job/log" "$tailn"
    return 1
  fi
  if [ "$quiet" -eq 0 ] && [ -n "$started_at" ]; then
    show_tail "$job/log" "$tailn"
  fi
  if [ "$rc" -eq 0 ]; then
    echo "JOB $name DONE rc=0 in ${elapsed}s  (log: $job/log)"
    return 0
  else
    echo "JOB $name FAILED rc=$rc after ${elapsed}s  (log: $job/log)"
    return 1
  fi
}

cmd_status() {
  [ "$#" -eq 1 ] || usage
  local name="$1"
  check_name "$name"
  local job; job="$(jobdir "$name")"
  [ -f "$job/pid" ] || die "no such job '$name' (state dir: $job)"
  local pid="" want="" rc=""
  read_state "$job"
  if alive "$pid" "$want"; then
    local started_at=""; [ -f "$job/started" ] && started_at="$(cat "$job/started")"
    echo "running (pid $pid, started $(date -d "@$started_at" '+%H:%M:%S' 2>/dev/null || echo $started_at))"
  elif [ -n "$rc" ]; then
    echo "finished rc=$rc"
  else
    echo "dead (no status recorded)"
  fi
}

cmd_tail() {
  [ "$#" -ge 1 ] || usage
  local name="$1"; shift
  local n="${1:-40}"
  local job; job="$(jobdir "$name")"
  [ -f "$job/log" ] || die "no log for job '$name'"
  tail -n "$n" "$job/log"
}

cmd_stop() {
  [ "$#" -ge 1 ] || usage
  local name="$1"; shift
  local grace=5
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --grace) [ "$#" -ge 2 ] || die "stop: --grace needs SECONDS"; grace="$2"; shift 2 ;;
      *) die "stop: unknown option '$1'" ;;
    esac
  done
  local job; job="$(jobdir "$name")"
  [ -f "$job/pid" ] || die "no such job '$name'"
  local pid="" want="" rc=""
  read_state "$job"
  if ! alive "$pid" "$want"; then
    echo "job '$name' is not running${rc:+ (final rc=$rc)}"
    return 0
  fi
  echo "stopping job '$name' (pid $pid): TERM to process group"
  kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
  local i=0 tries=$(( grace * 2 ))
  while alive "$pid" "$want" && [ "$i" -lt "$tries" ]; do sleep 0.5; i=$((i + 1)); done
  if alive "$pid" "$want"; then
    echo "  still alive after ${grace}s — KILLing process group"
    kill -KILL -- "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
    echo 137 > "$job/status"
    sleep 1
  fi
  echo "stopped '$name'"
}

cmd_clean() {
  [ "$#" -eq 1 ] || usage
  local name="$1"
  check_name "$name"
  local job; job="$(jobdir "$name")"
  [ -e "$job" ] || die "no such job '$name'"
  local pid="" want="" rc=""
  read_state "$job"
  if alive "$pid" "$want"; then
    die "job '$name' is still running (pid $pid) — 'stop' it first"
  fi
  rm -rf "$job"
  echo "cleaned job '$name'"
}

cmd_list() {
  local any=0
  for job in "$STATE"/*; do
    [ -d "$job" ] || continue
    local name; name="$(basename "$job")"
    local pid="" want="" rc="" state
    read_state "$job"
    if alive "$pid" "$want"; then
      state="running (pid $pid)"
    elif [ -n "$rc" ]; then
      state="finished rc=$rc"
    else
      state="dead (no status)"
    fi
    printf '%-24s %s   log %s lines\n' "$name" "$state" \
      "$(wc -l < "$job/log" 2>/dev/null | tr -d '[:space:]' || echo 0)"
    any=1
  done
  [ "$any" -eq 1 ] || echo "(no jobs — state dir: $STATE)"
}

# ---------------------------------------------------------------------------
# wait-file — salvage poll for log-only operations (no pidfile available)
# ---------------------------------------------------------------------------

cmd_wait_file() {
  [ "$#" -ge 2 ] || usage
  local logf="$1" regex="$2"; shift 2
  local timeout interval tailn quiet minidle
  parse_wait_opts wait-file "$@"
  [ -n "$regex" ] || die "wait-file: empty END_REGEX"
  [ -f "$logf" ] || die "wait-file: log file not found: $logf"
  local t0 deadline
  t0="$(now)"; deadline=$(( t0 + timeout ))
  local size=0 stable=0
  while :; do
    if grep -qE -- "$regex" "$logf" 2>/dev/null; then
      local s; s="$(stat -c %s "$logf" 2>/dev/null || echo 0)"
      if [ "$s" = "$size" ]; then stable=$((stable + 1)); else size="$s"; stable=0; fi
      if [ "$stable" -ge "$minidle" ]; then
        [ "$quiet" -eq 0 ] && show_tail "$logf" "$tailn"
        echo "LOG DONE after $(( $(now) - t0 ))s: '$regex' matched and stable (log: $logf)"
        return 0
      fi
    else
      size="$(stat -c %s "$logf" 2>/dev/null || echo 0)"
      stable=0
    fi
    if [ "$(now)" -ge "$deadline" ]; then
      echo "LOG STILL ACTIVE after ${timeout}s (no stable '$regex' match) — re-run wait-file to keep polling"
      return 2
    fi
    sleep "$interval"
  done
}

# ---------------------------------------------------------------------------
# dispatch
# ---------------------------------------------------------------------------

[ "$#" -ge 1 ] || usage
cmd="$1"; shift
case "$cmd" in
  start)     cmd_start "$@" ;;
  wait)      cmd_wait "$@" ;;
  status)    cmd_status "$@" ;;
  tail)      cmd_tail "$@" ;;
  stop)      cmd_stop "$@" ;;
  clean)     cmd_clean "$@" ;;
  list)      cmd_list "$@" ;;
  wait-file) cmd_wait_file "$@" ;;
  *) die "unknown command '$cmd' (start|wait|status|tail|stop|clean|list|wait-file)" ;;
esac
