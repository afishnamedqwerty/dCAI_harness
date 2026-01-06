#!/bin/bash
# Long-Running Agent Orchestrator (Ralph-Style)
# Runs Claude Code in a loop until PRD is complete or max iterations reached

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/progress.sh"
source "$SCRIPT_DIR/lib/ci_guard.sh"
source "$SCRIPT_DIR/lib/detect_runner.sh"

# Configuration
MAX_ITERATIONS="${MAX_ITERATIONS:-10}"
PRD_FILE="${1:-prd.json}"
PROGRESS_FILE="${PROGRESS_FILE:-progress.txt}"
WORK_DIR="${WORK_DIR:-.}"
DRY_RUN="${DRY_RUN:-false}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Print usage
usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS] [PRD_FILE]

Long-running agent orchestrator using Claude Code.

Arguments:
    PRD_FILE            Path to PRD JSON file (default: prd.json)

Options:
    --max-iterations N  Maximum loop iterations (default: 10)
    --work-dir DIR      Working directory for agent (default: current)
    --progress FILE     Progress file path (default: progress.txt)
    --dry-run           Run without executing Claude (for testing)
    -h, --help          Show this help message

Environment Variables:
    MAX_ITERATIONS      Same as --max-iterations
    PROGRESS_FILE       Same as --progress
    WORK_DIR            Same as --work-dir
    DRY_RUN             Set to 'true' for dry run mode

Examples:
    ./orchestrate.sh my_prd.json
    MAX_ITERATIONS=5 ./orchestrate.sh --work-dir ./my-project prd.json
EOF
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --max-iterations)
            MAX_ITERATIONS="$2"
            shift 2
            ;;
        --work-dir)
            WORK_DIR="$2"
            shift 2
            ;;
        --progress)
            PROGRESS_FILE="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN="true"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            log_error "Unknown option: $1"
            usage
            exit 1
            ;;
        *)
            PRD_FILE="$1"
            shift
            ;;
    esac
done

# Validate PRD file exists
if [[ ! -f "$PRD_FILE" ]]; then
    log_error "PRD file not found: $PRD_FILE"
    exit 1
fi

# Initialize
log_info "Starting orchestrator with PRD: $PRD_FILE"
log_info "Max iterations: $MAX_ITERATIONS"
log_info "Working directory: $WORK_DIR"

cd "$WORK_DIR"

# Detect CI commands
detect_test_runner
TEST_CMD="$DETECTED_TEST_CMD"
BUILD_CMD="$DETECTED_BUILD_CMD"
log_info "Detected test command: ${TEST_CMD:-none}"
log_info "Detected build command: ${BUILD_CMD:-none}"

# Load prompt template
PROMPT_TEMPLATE=$(cat "$SCRIPT_DIR/prompts/agent_prompt.md")

# Check if all PRD items pass
check_prd_complete() {
    local prd_content
    prd_content=$(cat "$PRD_FILE")
    
    # Count items with "passes": false
    local failing_items
    failing_items=$(echo "$prd_content" | jq '[.features[] | select(.passes == false)] | length')
    
    if [[ "$failing_items" == "0" ]]; then
        return 0
    else
        return 1
    fi
}

# Build the agent prompt with current context
build_prompt() {
    local prd_json
    local progress_content
    local prompt
    
    prd_json=$(cat "$PRD_FILE")
    
    if [[ -f "$PROGRESS_FILE" ]]; then
        progress_content=$(tail -50 "$PROGRESS_FILE" 2>/dev/null || echo "No previous progress.")
    else
        progress_content="No previous progress."
    fi
    
    # Replace placeholders in template
    prompt="$PROMPT_TEMPLATE"
    prompt="${prompt//\{\{PRD_JSON\}\}/$prd_json}"
    prompt="${prompt//\{\{PROGRESS_TXT\}\}/$progress_content}"
    prompt="${prompt//\{\{TEST_COMMAND\}\}/${TEST_CMD:-echo 'No tests configured'}}"
    prompt="${prompt//\{\{BUILD_COMMAND\}\}/${BUILD_CMD:-echo 'No build configured'}}"
    
    echo "$prompt"
}

# Run a single agent iteration
run_agent_iteration() {
    local iteration=$1
    local prompt
    
    log_info "=== Iteration $iteration of $MAX_ITERATIONS ==="
    
    # Build prompt with current context
    prompt=$(build_prompt)
    
    if [[ "$DRY_RUN" == "true" ]]; then
        log_warn "DRY RUN: Would execute Claude with prompt (truncated):"
        echo "${prompt:0:500}..."
        
        # In dry run, check if PRD is complete
        if check_prd_complete; then
            echo "<promise>COMPLETE</promise>"
            return 0
        fi
        return 0
    fi
    
    # Execute Claude Code
    local output
    local exit_code=0
    
    output=$(claude -p "$prompt" \
        --output-format text \
        --dangerously-skip-permissions \
        2>&1) || exit_code=$?
    
    if [[ $exit_code -ne 0 ]]; then
        log_error "Claude execution failed with exit code: $exit_code"
        log_error "Output: $output"
        return 1
    fi
    
    # Check for completion signal
    if echo "$output" | grep -q "<promise>COMPLETE</promise>"; then
        log_success "Agent signaled completion!"
        return 2  # Special exit code for completion
    fi
    
    # Run CI guard before allowing any commits
    log_info "Running CI guard..."
    if ! run_ci_guard "$TEST_CMD" "$BUILD_CMD"; then
        log_error "CI guard failed! Reverting last commit..."
        git reset --hard HEAD~1 2>/dev/null || true
        append_progress "[$iteration] CI FAILED - commit reverted"
        return 1
    fi
    
    log_success "CI guard passed!"
    
    # Append progress entry
    append_progress "[$iteration] Iteration completed successfully"
    
    return 0
}

# Main orchestration loop
main() {
    local iteration=1
    local result
    
    # Check if already complete
    if check_prd_complete; then
        log_success "All PRD items already passing!"
        echo "<promise>COMPLETE</promise>"
        exit 0
    fi
    
    # Touch progress file
    touch "$PROGRESS_FILE"
    append_progress "[START] Orchestrator started with PRD: $PRD_FILE"
    
    while [[ $iteration -le $MAX_ITERATIONS ]]; do
        run_agent_iteration $iteration
        result=$?
        
        case $result in
            0)  # Normal completion
                ;;
            2)  # Agent signaled complete
                log_success "Orchestrator finished: Agent completed all tasks"
                append_progress "[COMPLETE] All PRD items passing"
                exit 0
                ;;
            *)  # Error
                log_warn "Iteration $iteration had issues, continuing..."
                ;;
        esac
        
        # Re-check PRD after each iteration
        if check_prd_complete; then
            log_success "All PRD items now passing!"
            append_progress "[COMPLETE] All PRD items verified passing"
            echo "<promise>COMPLETE</promise>"
            exit 0
        fi
        
        ((iteration++))
    done
    
    log_warn "Reached maximum iterations ($MAX_ITERATIONS) without completion"
    append_progress "[MAX_ITER] Stopped after $MAX_ITERATIONS iterations"
    exit 1
}

main "$@"
