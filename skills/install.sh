#!/usr/bin/env bash
# Sync bundled Forge skills to the canonical ~/.agents/skills hub and link/sync
# them into detected agent skill directories (Antigravity/Gemini, Codex, Claude, Cursor).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CANONICAL_HUB="${AGENTS_SKILLS_DIR:-$HOME/.agents/skills}"
MODE="link"
DRY_RUN=0
SELECTED_SKILLS=()

usage() {
    cat <<'EOF'
Usage: ./skills/install.sh [OPTIONS] [SKILL...]

Syncs bundled skills from skills/ into ~/.agents/skills and updates typical
global agent skill directories (~/.gemini/config/skills, ~/.codex/skills,
~/.claude/skills, ~/.cursor/skills).

Options:
  --copy       Copy files into each agent directory instead of symlinking to ~/.agents/skills
  --dry-run    Print planned actions without modifying disk
  -h, --help   Show this help message
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --copy)
            MODE="copy"
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
        *)
            SELECTED_SKILLS+=("$1")
            shift
            ;;
    esac
done

run_cmd() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "[dry-run] $*"
    else
        "$@"
    fi
}

sync_dir() {
    local src="$1"
    local dst="$2"
    if command -v rsync >/dev/null 2>&1; then
        run_cmd mkdir -p "$dst"
        run_cmd rsync -a --delete "$src/" "$dst/"
    else
        run_cmd rm -rf "$dst"
        run_cmd mkdir -p "$dst"
        run_cmd cp -a "$src/." "$dst/"
    fi
}

if [[ ${#SELECTED_SKILLS[@]} -eq 0 ]]; then
    for dir in "$SCRIPT_DIR"/*/; do
        [[ -d "$dir" ]] || continue
        if [[ -f "${dir}SKILL.md" ]]; then
            SELECTED_SKILLS+=("$(basename "$dir")")
        fi
    done
fi

if [[ ${#SELECTED_SKILLS[@]} -eq 0 ]]; then
    echo "No skills found in $SCRIPT_DIR" >&2
    exit 1
fi

run_cmd mkdir -p "$CANONICAL_HUB"
if [[ -f "$SCRIPT_DIR/README.md" ]]; then
    run_cmd cp -f "$SCRIPT_DIR/README.md" "$CANONICAL_HUB/README.md"
fi

for skill in "${SELECTED_SKILLS[@]}"; do
    src_dir="$SCRIPT_DIR/$skill"
    if [[ ! -f "$src_dir/SKILL.md" ]]; then
        echo "Error: skill '$skill' not found at $src_dir/SKILL.md" >&2
        exit 1
    fi
    dst_dir="$CANONICAL_HUB/$skill"
    if [[ -L "$dst_dir" ]]; then
        run_cmd rm -f "$dst_dir"
    fi
    sync_dir "$src_dir" "$dst_dir"
    echo "[hub]     synced $skill -> $dst_dir"
done

AGENT_TARGETS=(
    "gemini:$HOME/.gemini:$HOME/.gemini/config/skills"
    "codex:$HOME/.codex:$HOME/.codex/skills"
    "claude:$HOME/.claude:$HOME/.claude/skills"
    "cursor:$HOME/.cursor:$HOME/.cursor/skills"
)

for entry in "${AGENT_TARGETS[@]}"; do
    IFS=':' read -r agent_name agent_root agent_skills_dir <<<"$entry"
    [[ -d "$agent_root" ]] || continue
    run_cmd mkdir -p "$agent_skills_dir"

    for skill in "${SELECTED_SKILLS[@]}"; do
        hub_skill="$CANONICAL_HUB/$skill"
        target_skill="$agent_skills_dir/$skill"

        if [[ "$MODE" == "link" && ( ! -e "$target_skill" || -L "$target_skill" ) ]]; then
            run_cmd ln -sfn "$hub_skill" "$target_skill"
            echo "[$agent_name] linked $skill -> $target_skill"
        else
            if [[ -L "$target_skill" ]]; then
                run_cmd rm -f "$target_skill"
            fi
            sync_dir "$hub_skill" "$target_skill"
            echo "[$agent_name] copied $skill -> $target_skill"
        fi
    done
done
