# Next Session Quick Start

**Previous Session**: Foundation & Bridge Complete  
**Status**: Ready for Execution Integration  
**Estimated Time for Next Phase**: 2-3 hours

## What's Done ✅

Three clean commits:
```
429926e docs: Add Kaptaind integration progress report
20f288e feat: Add Kaptaind bridge functions to UNI revise module  
90e69cd feat: Establish Kaptaind transaction model and core infrastructure
```

**Deliverables**:
- `src/kaptaind.rs` (600 lines) - Complete transaction system
- Bridge functions in `src/revise.rs` - Plan generation ready to use
- Full documentation in `docs/kaptaind-integration-progress.md`

## What's Next 🎯

**Critical Path (Pick One)**:

### Option A: Complete Execution Integration (30-45 min)
**Goal**: Make `uni revise --apply` work in dirty worktrees

**File to modify**: `src/revise.rs` (execute() function, line 348)

**Changes needed**:
1. Remove dirty_worktree check (lines 371-376)
2. After building plan, call: `build_kaptaind_plan(&diagnosis, &target, &remediations)`
3. Check staleness: `check_plan_staleness(&target, &plan)`
4. For now, keep existing execution but mark it as "legacy path"
5. Add TODO comment for Kaptaind execution delegation

**Test**: `cargo build && cargo test` (should compile, existing tests pass)

**Result**: Foundation ready for next phase (rollback mechanics)

---

### Option B: Write Integration Tests (1-2 hours)
**Goal**: Validate all 13 scenarios from the directive

**Files to create**: `tests/kaptaind_integration.rs`

**Test coverage**:
```rust
#[test]
async fn test_clean_repo_remediation()        // Clean path
async fn test_dirty_repo_preservation()       // User changes preserved
async fn test_overlapping_modifications()     // Conflict detection
async fn test_tool_unavailable()              // Graceful skip
async fn test_remediation_failure()           // Rollback
async fn test_stale_plan_detection()          // Re-analysis
async fn test_remediation_branch_naming()     // Deterministic names
// ... etc
```

**Result**: Comprehensive coverage, validates assumptions

---

### Option C: Transaction Persistence (45-60 min)
**Goal**: Save transaction state to disk for recovery

**Files to create**: `src/kaptaind_persistence.rs`

**Needed functions**:
```rust
pub async fn save_transaction(txn: &RemediationTransaction) -> Result<(), String>
pub async fn load_transaction(txn_id: &TransactionId) -> Result<RemediationTransaction, String>
pub async fn list_transactions() -> Result<Vec<TransactionId>, String>
```

**Storage**: `.kaptaind/transactions/{id}.json`

**Result**: Safe to kill process mid-remediation

---

## Code Locations Reference

### Key Files
```
src/kaptaind.rs                       # Core transaction system (600 lines)
  ├─ RemediationPlan               # What to fix
  ├─ RemediationTransaction        # Transaction state machine
  ├─ WorktreePreservation          # User state tracking
  ├─ preserve_worktree()           # Safety function
  └─ check_plan_staleness()        # Validation

src/revise.rs                         # UNI revise logic
  ├─ execute()                     # Main entry point (needs integration)
  ├─ build_kaptaind_plan()         # Bridge function (line ~624)
  ├─ check_plan_staleness()        # Bridge function (line ~702)
  └─ check_worktree()              # Needs to be replaced (line ~1184)

docs/kaptaind-integration-progress.md # Full technical documentation
```

### Key Structures
```rust
// In src/kaptaind.rs:
pub struct RemediationPlan {
    pub plan_id: String,
    pub analysis_fingerprint: AnalysisFingerprint,
    pub remediations: Vec<PlannedRemediation>,
}

pub struct RemediationTransaction {
    pub transaction_id: TransactionId,
    pub state: TransactionState,  // State machine
    pub worktree_preservation: Option<WorktreePreservation>,
    pub remediation_branch: Option<String>,
    pub results: Vec<RemediationResult>,
}
```

## Compilation Status

✅ **Currently compiles clean**
```
cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.36s
```

2 expected warnings (dead code in bridge functions - will be used in execute())

## Running Tests

```bash
# Current tests
cargo test --lib

# After adding integration tests
cargo test --all

# Specific module
cargo test kaptaind
```

## Git Commands for Reference

```bash
# See what's changed this session
git log --oneline -3

# Show full diff of latest commit
git show 20f288e

# Check current branch
git branch

# Create working branch for next phase
git checkout -b feature/kaptaind-execution
```

## Debugging Tips

**Compilation errors**:
```bash
cargo check        # Quick check without building
cargo build --verbose  # See full compiler output
```

**Testing with prints**:
```rust
// In code:
dbg!(&variable);  // Prints variable and returns it
eprintln!("debug: {}", value);  // Write to stderr

// Run test with output:
cargo test -- --nocapture
```

**Git history**:
```bash
git log --stat         # See files changed
git diff HEAD~1        # Show changes in latest commit
git show --name-only   # List files in commit
```

## Common Errors & Fixes

| Error | Cause | Fix |
|-------|-------|-----|
| `no space left on device` | Disk full (target/ too big) | `cargo clean` |
| `E0599: no variant named X` | Enum name mismatch | Check kaptaind.rs for RemediationClass variants |
| `cannot find function` | Not exported or in wrong module | Add `pub` or check module path |
| `type mismatch` | Parameter type wrong | Check function signature |

## Success Criteria for Next Phase

**If Option A (Execution Integration)**:
- [ ] execute() builds successfully
- [ ] Existing revise tests still pass
- [ ] build_kaptaind_plan() is called (can add debug print)
- [ ] No dirty_worktree error on second path

**If Option B (Integration Tests)**:
- [ ] All 13 test scenarios compile
- [ ] At least 5 pass
- [ ] Integration test framework is in place

**If Option C (Transaction Persistence)**:
- [ ] save_transaction() writes JSON to disk
- [ ] load_transaction() reads it back
- [ ] Transactions survive process restart

## Questions to Ask Before Starting

1. Do you want to complete execution integration first (critical path)?
2. Or add tests for confidence?
3. Or persist transactions for recovery?

All are valid. Pick the one that feels most important for your goals.

---

**Remember**: The foundation is solid. The bridge is built. The architecture is sound. The next phase is just wiring it all together.

Good luck! 🚀
