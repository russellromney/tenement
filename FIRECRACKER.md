# Firecracker VM Runtime Implementation Plan

**Status:** Complete (untested on KVM)
**Target:** tenement 0.2.0

## Implementation Status

| Phase | Status | Description |
|-------|--------|-------------|
| Phase 1: Runtime Trait | ✅ Complete | `Runtime` trait, `RuntimeHandle`, `ProcessRuntime` |
| Phase 2: Config Extensions | ✅ Complete | `runtime`, `kernel`, `rootfs`, `memory_mb`, `vcpus`, `vsock_port` fields |
| Phase 3: Firecracker Runtime | ✅ Complete | Full VM spawn via HTTP API, lifecycle management |
| Phase 4: VSOCK Integration | ✅ Complete | Health checks support vsock CONNECT protocol |
| Phase 5: Documentation | ✅ Complete | This file and README updated |

**What works now:**
- Config parsing for Firecracker processes
- Config validation (requires kernel + rootfs)
- Runtime trait abstraction
- VSOCK-aware health checks
- Clear error messages on unsupported platforms
- **VM spawning via Firecracker REST API**
- **VM lifecycle management (stop with graceful shutdown)**

**What's TODO:**
- Integration tests on KVM-enabled systems (code is complete but untested on real hardware)

## Overview

Add Firecracker microVM as a second runtime option. Same routing, same supervision, same API - different isolation. "Some tenants get curtains, some get walls."

## Platform Requirements

Firecracker requires KVM (bare metal or nested virtualization).

| Platform | Process Runtime | Firecracker Runtime |
|----------|-----------------|---------------------|
| Bare metal Linux | ✅ | ✅ |
| VPS with KVM | ✅ | ✅ |
| AWS EC2 metal | ✅ | ✅ |
| GCP (nested virt) | ✅ | ✅ |
| Fly.io | ✅ | ❌ (no nested virt) |
| AWS EC2 regular | ✅ | ❌ |
| macOS | ✅ | ❌ (no KVM) |

## Configuration

```toml
# Process runtime (default, existing behavior)
[process.api]
command = "uv run python app.py"
socket = "/tmp/tenement/api-{id}.sock"
health = "/health"

# Firecracker runtime
[process.secure-worker]
runtime = "firecracker"
kernel = "/var/lib/tenement/vmlinux"
rootfs = "/var/lib/tenement/worker.ext4"
socket = "/tmp/tenement/secure-{id}.sock"
health = "/health"
memory_mb = 256
vcpus = 2
vsock_port = 5000

[process.secure-worker.env]
DATABASE_URL = "{data_dir}/{id}/app.db"
```

### New Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `runtime` | string | `"process"` | `"process"` or `"firecracker"` |
| `kernel` | path | required | Path to vmlinux kernel image |
| `rootfs` | path | required | Path to root filesystem (ext4) |
| `memory_mb` | u32 | 128 | VM memory in MB |
| `vcpus` | u8 | 1 | Number of virtual CPUs |
| `vsock_port` | u32 | 5000 | Guest vsock port for service |

## Architecture

### Runtime Trait

```rust
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Spawn a new instance
    async fn spawn(
        &self,
        config: &RuntimeConfig,
        id: &str,
        env: HashMap<String, String>,
    ) -> Result<RuntimeHandle>;

    /// Stop a running instance
    async fn stop(&self, handle: &RuntimeHandle) -> Result<()>;

    /// Check if instance is still running
    async fn is_alive(&self, handle: &RuntimeHandle) -> bool;

    /// Runtime type identifier
    fn runtime_type(&self) -> RuntimeType;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeType {
    Process,
    Firecracker,
}
```

### VSOCK Bridging

Firecracker exposes guest services via vsock → Unix socket:

```
Host                          Firecracker                    Guest
─────                         ───────────                    ─────
connect(fc-id-vsock.sock)  →
send("CONNECT 5000\n")     →
                           ←  recv("OK 5000\n")
                              [socket bridged to guest:5000]
send(HTTP request)         →                              →  recv(HTTP request)
                           ←                              ←  send(HTTP response)
recv(HTTP response)        ←
```

Existing health checks and routing work unchanged - they just go through the vsock bridge.

## File Structure

```
tenement/tenement/src/
├── runtime/
│   ├── mod.rs           # Runtime trait, RuntimeType, RuntimeConfig
│   ├── process.rs       # ProcessRuntime (extracted from hypervisor)
│   └── firecracker.rs   # FirecrackerRuntime
├── hypervisor.rs        # Modified to use Runtime trait
├── config.rs            # Extended with Firecracker fields
└── lib.rs               # Export runtime module
```

## Implementation Phases

### Phase 1: Runtime Trait Abstraction

Extract existing process spawning into a trait. No functional changes.

**Files:**
- NEW: `tenement/tenement/src/runtime/mod.rs`
- NEW: `tenement/tenement/src/runtime/process.rs`
- MODIFY: `tenement/tenement/src/hypervisor.rs`
- MODIFY: `tenement/tenement/src/lib.rs`

**Changes:**
1. Create `Runtime` trait with `spawn`, `stop`, `is_alive`, `runtime_type`
2. Create `RuntimeHandle` struct (replaces direct `Child` access)
3. Extract process spawning logic from `Hypervisor::spawn` into `ProcessRuntime`
4. Hypervisor uses `ProcessRuntime` via trait (behavior unchanged)

**Verification:** All existing tests pass.

### Phase 2: Config Extensions

Add Firecracker-specific configuration fields.

**Files:**
- MODIFY: `tenement/tenement/src/config.rs`

**Changes:**
1. Add `RuntimeType` enum with serde support
2. Add fields: `runtime`, `memory_mb`, `vcpus`, `kernel`, `rootfs`, `vsock_port`
3. Add `ProcessConfig::validate()` - Firecracker requires kernel + rootfs
4. Add `ProcessConfig::to_vm_config()` helper

**New Tests:**
```rust
#[test]
fn test_firecracker_config_requires_kernel() {
    let config = ProcessConfig {
        runtime: RuntimeType::Firecracker,
        rootfs: Some("/path/to/rootfs".into()),
        kernel: None, // Missing!
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_firecracker_config_valid() {
    let config_str = r#"
[process.api]
runtime = "firecracker"
kernel = "/path/to/vmlinux"
rootfs = "/path/to/rootfs.ext4"
memory_mb = 256
vcpus = 2
"#;
    let config: Config = toml::from_str(config_str).unwrap();
    assert!(config.get_process("api").unwrap().validate().is_ok());
}
```

### Phase 3: Firecracker Runtime

Implement the Firecracker runtime using `firepilot` crate.

**Files:**
- NEW: `tenement/tenement/src/runtime/firecracker.rs`
- MODIFY: `tenement/tenement/Cargo.toml`

**Dependencies:**
```toml
[dependencies]
firepilot = { version = "1.2", optional = true }
async-trait = "0.1"

[features]
default = []
firecracker = ["firepilot"]
```

**Implementation:**
```rust
pub struct FirecrackerRuntime {
    firecracker_bin: PathBuf,
    socket_dir: PathBuf,
    vms: RwLock<HashMap<String, VmInstance>>,
}

impl FirecrackerRuntime {
    pub fn new(firecracker_bin: PathBuf, socket_dir: PathBuf) -> Result<Self> {
        // Check /dev/kvm exists
        if !PathBuf::from("/dev/kvm").exists() {
            anyhow::bail!(
                "Firecracker requires KVM. /dev/kvm not found. \
                 Running on a VM without nested virtualization?"
            );
        }
        // Check firecracker binary
        if !firecracker_bin.exists() {
            anyhow::bail!("Firecracker binary not found at {:?}", firecracker_bin);
        }
        Ok(Self { ... })
    }

    pub fn is_available() -> bool {
        PathBuf::from("/dev/kvm").exists()
    }
}
```

**VM Lifecycle:**
1. Spawn `firecracker --api-sock /path/to/api.sock`
2. Wait for API socket
3. Configure VM via REST API:
   - PUT /boot-source (kernel + cmdline)
   - PUT /machine-config (vcpus, memory)
   - PUT /drives/rootfs (root filesystem)
   - PUT /vsock (vsock device with Unix socket path)
4. PUT /actions `{"action_type": "InstanceStart"}`
5. Return `RuntimeHandle` with vsock socket path

**Stop:**
1. Kill firecracker process
2. Clean up API socket and vsock socket

### Phase 4: VSOCK Integration

Make health checks and routing work through vsock.

**Files:**
- MODIFY: `tenement/tenement/src/hypervisor.rs`
- MODIFY: `tenement/cli/src/server.rs`

**Health Check Modification:**
```rust
async fn ping_health(
    &self,
    socket_path: &PathBuf,
    endpoint: &str,
    vsock_port: Option<u32>,  // New parameter
) -> Result<()> {
    let mut stream = UnixStream::connect(socket_path).await?;

    // If vsock, send CONNECT protocol
    if let Some(port) = vsock_port {
        stream.write_all(format!("CONNECT {}\n", port).as_bytes()).await?;
        let mut buf = [0u8; 32];
        let n = stream.read(&mut buf).await?;
        if !String::from_utf8_lossy(&buf[..n]).starts_with("OK ") {
            anyhow::bail!("vsock CONNECT failed");
        }
    }

    // Standard HTTP health check
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        endpoint
    );
    stream.write_all(request.as_bytes()).await?;
    // ... read response
}
```

**Router Modification:**
Same pattern - detect if socket is vsock and send CONNECT before proxying.

### Phase 5: Documentation

**Files:**
- MODIFY: `tenement/README.md` - Add Runtimes section (already done)
- NEW: `tenement/docs/firecracker-setup.md` - Kernel/rootfs preparation guide

**Firecracker Setup Guide:**
1. Download Firecracker binary
2. Build or download kernel (vmlinux)
3. Create rootfs with your application
4. Configure tenement.toml
5. Run with `ten spawn`

## Testing Strategy

### Unit Tests (No KVM Required)

```rust
#[test]
fn test_runtime_type_serde() {
    assert_eq!(
        serde_json::to_string(&RuntimeType::Firecracker).unwrap(),
        "\"firecracker\""
    );
}

#[test]
fn test_process_runtime_trait_impl() {
    // ProcessRuntime implements Runtime trait
    let rt: &dyn Runtime = &ProcessRuntime::new();
    assert_eq!(rt.runtime_type(), RuntimeType::Process);
}
```

### Integration Tests (Require KVM)

```rust
#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn test_firecracker_spawn_stop() {
    if !FirecrackerRuntime::is_available() {
        eprintln!("Skipping: KVM not available");
        return;
    }
    // ... test VM lifecycle
}
```

### Graceful Degradation Tests

```rust
#[tokio::test]
async fn test_firecracker_unavailable_clear_error() {
    // When user requests Firecracker but KVM unavailable
    let hypervisor = Hypervisor::new(config_with_firecracker_process());

    if !FirecrackerRuntime::is_available() {
        let result = hypervisor.spawn("secure", "test").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Firecracker") || err.contains("KVM"));
    }
}
```

## Future Considerations

### Resource Limits (cgroups)
Process runtime could also support memory/CPU limits via cgroups, independent of Firecracker.

### WASM Runtime
A third runtime option using wasmtime/wasmer for sandboxed execution without VM overhead.

### Hibernation
Both runtimes could support snapshot/restore for scale-to-zero.

## References

- [Firecracker GitHub](https://github.com/firecracker-microvm/firecracker)
- [Firecracker vsock docs](https://github.com/firecracker-microvm/firecracker/blob/main/docs/vsock.md)
- [firepilot Rust SDK](https://github.com/rik-org/firepilot)
- [firecracker-rs-sdk](https://crates.io/crates/firecracker-rs-sdk)
- [Fly.io nested virt discussion](https://community.fly.io/t/nested-virtualization-on-fly-io/11778)
