# Tenement Test Plan

Comprehensive test coverage improvement. **Result: 130 → 256 tests (+97%)**

## Progress Summary

| Category | Before | After | Added |
|----------|--------|-------|-------|
| Hypervisor | 2 | 32 | +30 |
| Instance | 22 | 48 | +26 |
| Cgroup | 4 | 26 | +22 |
| Runtime/Process | 4 | 20 | +16 |
| Runtime/mod | 5 | 5 | +0 |
| Logs | 10 | 45 | +35 |
| Store | 6 | 34 | +28 |
| Auth | 4 | 22 | +18 |
| Config | 39 | 39 | +0 |
| Metrics | 9 | 9 | +0 |
| **TOTAL** | **130** | **256** | **+126** |

## Tests Added by Category

### Hypervisor Tests (+30)
- Instance lifecycle: spawn, stop, list, get, restart
- Error paths: unknown process, command not found, already running
- Backoff: overflow protection, zero base, custom settings
- Activity tracking: touch_activity
- Log capture: stdout/stderr capture
- Metrics: spawn/stop increments counters
- Health status: various health check scenarios
- Data directory: creation on spawn
- Environment variables: custom env, SOCKET_PATH

### Instance Tests (+26)
- InstanceId: parse edge cases (empty, special chars, roundtrip)
- HealthStatus: all variants, copy, serialize
- InstanceStatus: all variants, copy, serialize
- InstanceInfo: serialization, clone, debug
- Uptime formatting: seconds, minutes, hours, days, boundaries
- Idle logic: timeout behavior

### Cgroup Tests (+22)
- ResourceLimits: all combinations, zero values, large values
- CgroupManager: path generation, special chars
- Linux tests: create/remove, memory limits, CPU weights (marked #[ignore])
- CPU weight clamping logic
- Memory bytes calculation

### Runtime/Process Tests (+16)
- Basic: availability, type, name, default
- Spawn: with args, env, workdir, removes old socket
- Errors: command not found
- Handle: socket, pid, is_running, kill
- Rapid spawn: multiple concurrent processes
- Exit: natural exit, error exit

### Logs Tests (+35)
- LogLevel: display, equality, copy, serialize
- LogEntry: timestamp, clone, serialize, empty/long messages, special chars
- RingBuffer: exact capacity, single capacity, empty, eviction
- Query filters: combined filters, no match, limit edge cases
- Search: case sensitive, empty string, no match
- LogBuffer async: len, is_empty, multiple subscribers, capacity

### Store Tests (+28)
- Database init: tables, FTS, indexes, idempotent
- Log insert: multiple, timestamp preservation
- Query: all filter combinations, limit, empty
- FTS search: no match, with filters, special chars
- Rotation: keeps recent, count
- ConfigStore: multiple keys, delete nonexistent, empty/long values, special chars
- Timestamp conversion: roundtrip, invalid input

### Auth Tests (+18)
- Token generation: uniqueness (100 tokens), URL-safe, length, entropy
- Hash: different hashes, format, malformed hashes, empty/long/unicode
- Verify: case sensitive
- TokenStore: set, replace, verify no token, clear idempotent, unique generation

## Running Tests

```bash
# All tests
cd tenement/tenement && cargo test

# With sandbox feature
cargo test --features sandbox

# Specific module
cargo test hypervisor::

# Show test count
cargo test 2>&1 | grep "test result"
```

## Test Design Principles

1. **Real behavior, no mocking** - Tests use actual processes (`sleep`, `echo`, `env`)
2. **Real files** - TempDir for actual file operations
3. **Edge cases** - Empty strings, long strings, special characters, boundaries
4. **Error paths** - Command not found, invalid input, nonexistent resources
5. **Platform awareness** - Linux-specific tests marked `#[cfg(target_os = "linux")]` or `#[ignore]`

## Future Improvements

- More integration tests (end-to-end lifecycle)
- Stress tests (concurrent operations)
- VM runtime tests (Firecracker/QEMU) - require actual binaries
- Namespace runtime tests - require root
- Network tests - health check via HTTP
