#!/bin/sh
# PreToolUse hook (Bash): Bazel is the only build system on this machine.
# Denies any command that runs cargo as a command word.
cmd=$(jq -r '.tool_input.command // ""')
# cargo in command position: at the start, after ; & | ( ` or $(, optionally
# after env assignments and wrappers such as time, env, exec, xargs.
re='(^|[;&|(`]|\$\()[[:space:]]*(([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]*|time|env|exec|nice|nohup|command|xargs|sudo)[[:space:]]+)*([~.]?/[^[:space:]]*/)?cargo([[:space:]]|$)'
if printf '%s\n' "$cmd" | grep -Eq "$re"; then
  jq -n '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny",
    permissionDecisionReason: "cargo is banned on this machine: Bazel is the only build system. Use scripts/bz (or bazel) to build, test and run, e.g. scripts/bz build //..., scripts/bz test //..., scripts/bz run //crates/sunflower:sunflower -- demo. Cargo.toml and Cargo.lock stay as the manifests Bazel reads through crate.from_cargo."}}'
fi
exit 0
