#!/bin/sh
# Bind configured ROM and save folders into Unslot's expected system folders.
# File syntax: ROM|source-folder|target-system or SAVE|source-core|target-system
# Mounts are intentionally opt-in. Never override a nonempty or mounted destination.
ROOT="${SLOT_ROOT:-/mnt/sdcard}"
CONFIG="$ROOT/Config/mounts.conf"
MODE="${1:---apply}"
case "$MODE" in --apply|--dry-run|--status) ;; *) echo "Usage: $0 --apply|--dry-run|--status"; exit 2 ;; esac
[ -f "$CONFIG" ] || exit 0
while IFS='|' read -r kind source dest extra || [ -n "$kind" ]; do
    case "$kind" in ''|\#*) continue ;; esac
    [ -z "$extra" ] || { echo "Invalid mount entry: extra fields"; continue; }
    # Use single folder names only. Never accept traversal or absolute paths.
    case "$source:$dest" in *..*|*/*|*\\*) echo "Invalid mount names: $kind"; continue ;; esac
    [ -n "$source" ] && [ -n "$dest" ] || { echo "Invalid mount entry: empty name"; continue; }
    case "$kind" in
      ROM) from="$ROOT/Roms/$source"; to="$ROOT/Games/$dest" ;;
      SAVE) from="$ROOT/Saves-Sync/saves/$source"; to="$ROOT/Saves/$dest" ;;
      *) echo "Unknown mount kind: $kind"; continue ;;
    esac
    [ -d "$from" ] || { echo "Missing source: $from"; continue; }
    [ -d "$to" ] || { echo "Missing destination: $to"; continue; }
    if awk -v target="$to" '$2 == target {found=1} END {exit !found}' /proc/mounts; then
        [ "$MODE" = "--status" ] && echo "MOUNTED $to"
        continue
    fi
    [ "$MODE" = "--status" ] && { echo "NOT MOUNTED $to"; continue; }
    if [ -n "$(ls -A "$to" 2>/dev/null)" ]; then
        echo "SKIP nonempty destination: $to"
        continue
    fi
    if [ "$MODE" = "--dry-run" ]; then
        echo "WOULD MOUNT $from -> $to"
    elif mount --bind "$from" "$to"; then
        echo "MOUNTED $from -> $to"
    else
        echo "FAILED $from -> $to"
    fi
done < "$CONFIG"
