#!/bin/bash
# Auto-detect test and build runners for the project

DETECTED_TEST_CMD=""
DETECTED_BUILD_CMD=""

detect_test_runner() {
    # Reset
    DETECTED_TEST_CMD=""
    DETECTED_BUILD_CMD=""
    
    # Node.js / npm
    if [[ -f "package.json" ]]; then
        # Check if test script exists
        if jq -e '.scripts.test' package.json > /dev/null 2>&1; then
            DETECTED_TEST_CMD="npm test"
        fi
        if jq -e '.scripts.build' package.json > /dev/null 2>&1; then
            DETECTED_BUILD_CMD="npm run build"
        fi
        # Check for typecheck
        if jq -e '.scripts.typecheck' package.json > /dev/null 2>&1; then
            DETECTED_BUILD_CMD="${DETECTED_BUILD_CMD:+$DETECTED_BUILD_CMD && }npm run typecheck"
        elif jq -e '.scripts["type-check"]' package.json > /dev/null 2>&1; then
            DETECTED_BUILD_CMD="${DETECTED_BUILD_CMD:+$DETECTED_BUILD_CMD && }npm run type-check"
        fi
        return 0
    fi
    
    # Rust / Cargo
    if [[ -f "Cargo.toml" ]]; then
        DETECTED_TEST_CMD="cargo test"
        DETECTED_BUILD_CMD="cargo build"
        # Add clippy if available
        if command -v cargo-clippy &> /dev/null || cargo clippy --version &> /dev/null; then
            DETECTED_BUILD_CMD="cargo clippy -- -D warnings && cargo build"
        fi
        return 0
    fi
    
    # Python / pytest
    if [[ -f "pytest.ini" ]] || [[ -f "pyproject.toml" ]] || [[ -f "setup.py" ]]; then
        # Check for pytest
        if [[ -f "pytest.ini" ]] || grep -q "pytest" pyproject.toml 2>/dev/null; then
            DETECTED_TEST_CMD="pytest"
        elif [[ -d "tests" ]]; then
            DETECTED_TEST_CMD="python -m pytest tests/"
        fi
        
        # Check for mypy
        if grep -q "mypy" pyproject.toml 2>/dev/null || [[ -f "mypy.ini" ]]; then
            DETECTED_BUILD_CMD="mypy ."
        fi
        
        # Check for ruff or flake8
        if grep -q "ruff" pyproject.toml 2>/dev/null; then
            DETECTED_BUILD_CMD="${DETECTED_BUILD_CMD:+$DETECTED_BUILD_CMD && }ruff check ."
        elif grep -q "flake8" pyproject.toml 2>/dev/null; then
            DETECTED_BUILD_CMD="${DETECTED_BUILD_CMD:+$DETECTED_BUILD_CMD && }flake8 ."
        fi
        return 0
    fi
    
    # Go
    if [[ -f "go.mod" ]]; then
        DETECTED_TEST_CMD="go test ./..."
        DETECTED_BUILD_CMD="go build ./..."
        return 0
    fi
    
    # Makefile fallback
    if [[ -f "Makefile" ]]; then
        if grep -q "^test:" Makefile; then
            DETECTED_TEST_CMD="make test"
        fi
        if grep -q "^build:" Makefile; then
            DETECTED_BUILD_CMD="make build"
        fi
        return 0
    fi
    
    # No runner detected
    return 1
}

# Export for use in other scripts
export -f detect_test_runner
export DETECTED_TEST_CMD
export DETECTED_BUILD_CMD
