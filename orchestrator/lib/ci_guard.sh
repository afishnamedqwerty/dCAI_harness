#!/bin/bash
# CI Guard - Ensures all tests pass before allowing commits

set -euo pipefail

CI_GUARD_LOG="${CI_GUARD_LOG:-/tmp/ci_guard.log}"

# Run CI checks and return success/failure
run_ci_guard() {
    local test_cmd="${1:-}"
    local build_cmd="${2:-}"
    local exit_code=0
    
    echo "=== CI Guard Starting ===" | tee "$CI_GUARD_LOG"
    echo "Timestamp: $(date -Iseconds)" | tee -a "$CI_GUARD_LOG"
    
    # Run build command if specified
    if [[ -n "$build_cmd" ]]; then
        echo "Running build: $build_cmd" | tee -a "$CI_GUARD_LOG"
        if eval "$build_cmd" >> "$CI_GUARD_LOG" 2>&1; then
            echo "✓ Build passed" | tee -a "$CI_GUARD_LOG"
        else
            echo "✗ Build FAILED" | tee -a "$CI_GUARD_LOG"
            exit_code=1
        fi
    fi
    
    # Run test command if specified
    if [[ -n "$test_cmd" ]]; then
        echo "Running tests: $test_cmd" | tee -a "$CI_GUARD_LOG"
        if eval "$test_cmd" >> "$CI_GUARD_LOG" 2>&1; then
            echo "✓ Tests passed" | tee -a "$CI_GUARD_LOG"
        else
            echo "✗ Tests FAILED" | tee -a "$CI_GUARD_LOG"
            exit_code=1
        fi
    fi
    
    # If neither command was specified, that's also a problem
    if [[ -z "$test_cmd" && -z "$build_cmd" ]]; then
        echo "⚠ Warning: No test or build commands configured" | tee -a "$CI_GUARD_LOG"
        # Don't fail - let the orchestrator decide what to do
        return 0
    fi
    
    echo "=== CI Guard Finished (exit: $exit_code) ===" | tee -a "$CI_GUARD_LOG"
    return $exit_code
}

# Verify last N commits all pass CI
verify_commits() {
    local n="${1:-1}"
    local current_sha
    current_sha=$(git rev-parse HEAD)
    
    echo "Verifying last $n commits..."
    
    for i in $(seq 0 $((n-1))); do
        local sha
        sha=$(git rev-parse HEAD~$i)
        echo "Checking commit: $sha"
        
        git checkout "$sha" --quiet
        if ! run_ci_guard "$DETECTED_TEST_CMD" "$DETECTED_BUILD_CMD"; then
            echo "Commit $sha failed CI!"
            git checkout "$current_sha" --quiet
            return 1
        fi
    done
    
    git checkout "$current_sha" --quiet
    echo "All $n commits pass CI!"
    return 0
}

# Export for use in other scripts
export -f run_ci_guard
export -f verify_commits
