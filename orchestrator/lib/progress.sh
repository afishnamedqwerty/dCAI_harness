#!/bin/bash
# Progress file management utilities

PROGRESS_FILE="${PROGRESS_FILE:-progress.txt}"

# Append a timestamped entry to progress.txt
append_progress() {
    local message="$1"
    local timestamp
    timestamp=$(date -Iseconds)
    
    echo "[$timestamp] $message" >> "$PROGRESS_FILE"
}

# Read the last N entries from progress.txt
read_progress() {
    local n="${1:-20}"
    
    if [[ ! -f "$PROGRESS_FILE" ]]; then
        echo "No progress file found."
        return 0
    fi
    
    tail -n "$n" "$PROGRESS_FILE"
}

# Get total number of entries
count_progress() {
    if [[ ! -f "$PROGRESS_FILE" ]]; then
        echo 0
        return 0
    fi
    
    wc -l < "$PROGRESS_FILE" | tr -d ' '
}

# Clear progress file (use with caution)
clear_progress() {
    if [[ -f "$PROGRESS_FILE" ]]; then
        local backup="${PROGRESS_FILE}.backup.$(date +%s)"
        cp "$PROGRESS_FILE" "$backup"
        echo "Backed up to: $backup"
    fi
    : > "$PROGRESS_FILE"
    echo "Progress file cleared."
}

# Search progress for a pattern
search_progress() {
    local pattern="$1"
    
    if [[ ! -f "$PROGRESS_FILE" ]]; then
        echo "No progress file found."
        return 1
    fi
    
    grep -i "$pattern" "$PROGRESS_FILE" || echo "No matches found."
}

# Export for use in other scripts
export -f append_progress
export -f read_progress
export -f count_progress
export -f clear_progress
export -f search_progress
export PROGRESS_FILE
