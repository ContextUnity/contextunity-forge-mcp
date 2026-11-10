#!/bin/sh
# Sync companion skills through the canonical hub into detected agent homes.
set -eu
SCRIPT_DIR=$(CDPATH= cd -P "$(dirname "$0")" && pwd)
CANONICAL_HUB=${AGENTS_SKILLS_DIR:-$HOME/.agents/skills}
MODE=link
DRY_RUN=0
usage() {
    echo 'Usage: ./skills/install.sh [--copy] [--dry-run] [SKILL...]'
    echo 'Sync bundled skills to ~/.agents/skills and detected agent directories.'
}
run_cmd() {
    if [ "$DRY_RUN" = 1 ]; then printf '[dry-run]'; printf ' %s' "$@"; printf '\n'; else "$@"; fi
}
same_dir() {
    [ -d "$1" ] && [ -d "$2" ] &&
        [ "$(CDPATH= cd -P "$1" && pwd -P)" = "$(CDPATH= cd -P "$2" && pwd -P)" ]
}
sync_dir() {
    if same_dir "$1" "$2"; then return; fi
    if command -v rsync >/dev/null 2>&1; then
        run_cmd mkdir -p "$2"
        run_cmd rsync -a --delete "$1/" "$2/"
    else
        run_cmd rm -rf "$2"
        run_cmd mkdir -p "$2"
        run_cmd cp -pR "$1/." "$2/"
    fi
}
# Retain selected names as positional arguments without eval or word splitting.
remaining=$#
while [ "$remaining" -gt 0 ]; do
    arg=$1; shift; remaining=$((remaining - 1))
    case $arg in
        --copy) MODE=copy ;;
        --dry-run) DRY_RUN=1 ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "Unknown option: $arg" >&2; exit 1 ;;
        *) set -- "$@" "$arg" ;;
    esac
done
if [ "$#" -eq 0 ]; then
    for dir in "$SCRIPT_DIR"/*/; do
        [ -f "${dir}SKILL.md" ] || continue
        name=${dir%/}; set -- "$@" "${name##*/}"
    done
fi
[ "$#" -gt 0 ] || { echo 'No bundled skills found' >&2; exit 1; }
for skill do
    case $skill in ''|.|..|*/*) echo "Invalid skill name: $skill" >&2; exit 1 ;; esac
    [ -f "$SCRIPT_DIR/$skill/SKILL.md" ] || { echo "Skill not found: $skill" >&2; exit 1; }
done
run_cmd mkdir -p "$CANONICAL_HUB"
if [ -f "$SCRIPT_DIR/README.md" ] && ! same_dir "$SCRIPT_DIR" "$CANONICAL_HUB"; then
    run_cmd cp -f "$SCRIPT_DIR/README.md" "$CANONICAL_HUB/README.md"
fi
for skill do
    dst=$CANONICAL_HUB/$skill
    if [ -L "$dst" ]; then run_cmd rm -f "$dst"; fi
    sync_dir "$SCRIPT_DIR/$skill" "$dst"
    printf '[hub] synced %s -> %s\n' "$skill" "$dst"
done
for agent in gemini codex claude cursor; do
    [ -d "$HOME/.$agent" ] || continue
    case $agent in gemini) target=$HOME/.gemini/config/skills ;; *) target=$HOME/.$agent/skills ;; esac
    run_cmd mkdir -p "$target"
    for skill do
        dst=$target/$skill
        if [ "$MODE" = link ] && { [ ! -e "$dst" ] || [ -L "$dst" ]; }; then
            if [ -L "$dst" ]; then run_cmd rm -f "$dst"; fi
            run_cmd ln -sf "$CANONICAL_HUB/$skill" "$dst"
        else
            if [ -L "$dst" ]; then run_cmd rm -f "$dst"; fi
            sync_dir "$CANONICAL_HUB/$skill" "$dst"
        fi
        printf '[%s] synced %s -> %s\n' "$agent" "$skill" "$dst"
    done
done
