# Long-Running Agent Orchestrator

A Ralph-style orchestrator for running Claude Code in a loop until a PRD is complete. Based on the pattern from `longrun.md`.

## Quick Start

```bash
# 1. Create a PRD for your project
cp templates/sample_prd.json my_project/prd.json
# Edit prd.json with your features

# 2. Run the orchestrator
cd orchestrator
./orchestrate.sh ../my_project/prd.json --work-dir ../my_project
```

## How It Works

```
┌─────────────────────────────────────────────────────────────────┐
│                        ORCHESTRATOR LOOP                         │
├─────────────────────────────────────────────────────────────────┤
│  1. Load PRD + progress.txt                                      │
│  2. Inject into system prompt                                    │
│  3. Run Claude Code                                              │
│  4. Agent picks highest-priority failing feature                 │
│  5. Agent implements, writes tests, commits                      │
│  6. CI Guard verifies tests pass                                 │
│  7. Agent updates PRD status                                     │
│  8. Append to progress.txt                                       │
│  9. Check: all features pass? → emit <promise>COMPLETE</promise> │
│ 10. Otherwise loop to step 1                                     │
└─────────────────────────────────────────────────────────────────┘
```

## Features

- **PRD-Driven**: JSON-based user stories with pass/fail tracking
- **CI Guard**: Auto-detects test runners, blocks commits that break tests
- **Progress Tracking**: Appends entries to `progress.txt` for context
- **Stop Condition**: `<promise>COMPLETE</promise>` when all PRD items pass
- **Auto-Recovery**: Reverts commits that fail CI

## PRD Format

```json
{
  "projectName": "My Project",
  "features": [
    {
      "id": "F001",
      "priority": 1,
      "title": "Feature Title",
      "description": "What this feature should do",
      "acceptanceCriteria": [
        "Criterion 1",
        "Criterion 2"
      ],
      "passes": false
    }
  ]
}
```

## Configuration

### Command Line Options

| Option | Description | Default |
|--------|-------------|---------|
| `--max-iterations N` | Maximum loop iterations | 10 |
| `--work-dir DIR` | Working directory for agent | current |
| `--progress FILE` | Progress file path | progress.txt |
| `--dry-run` | Run without executing Claude | false |

### Environment Variables

```bash
export MAX_ITERATIONS=5
export PROGRESS_FILE=my_progress.txt
export WORK_DIR=/path/to/project
export DRY_RUN=true
```

## Auto-Detected Test Runners

The orchestrator automatically detects:

| Project Type | Test Command | Build Command |
|--------------|--------------|---------------|
| Node.js | `npm test` | `npm run build` |
| Rust | `cargo test` | `cargo clippy && cargo build` |
| Python | `pytest` | `mypy . && ruff check .` |
| Go | `go test ./...` | `go build ./...` |
| Makefile | `make test` | `make build` |

Override with `ciConfig` in your PRD:
```json
{
  "ciConfig": {
    "testCommand": "custom test command",
    "buildCommand": "custom build command"
  }
}
```

## Files

```
orchestrator/
├── orchestrate.sh          # Main entry point
├── prompts/
│   └── agent_prompt.md     # System prompt template
├── templates/
│   ├── prd_schema.json     # JSON schema for PRD validation
│   └── sample_prd.json     # Example PRD
├── lib/
│   ├── detect_runner.sh    # Auto-detect test/build commands
│   ├── progress.sh         # Progress file management
│   └── ci_guard.sh         # CI-green enforcement
└── README.md               # This file
```

## Tips

1. **Start Small**: Begin with simple, well-scoped features
2. **Atomic Features**: Each feature should be implementable in one iteration
3. **Clear Acceptance Criteria**: The more specific, the better
4. **Watch Context**: If iterations are failing, features may be too large

## License

MIT
