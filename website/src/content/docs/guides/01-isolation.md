---
title: Isolation Levels
description: Process isolation security models
---

tenement provides multiple isolation levels for different security needs.

## The Spectrum

| Isolation | Tool | Overhead | Startup | Use Case |
|-----------|------|----------|---------|----------|
| **process** | bare | ~0 | <10ms | Same trust boundary, debugging |
| **namespace** | unshare | ~0 | <10ms | **Default** - trusted code, /proc isolated |
| **sandbox** | gVisor | ~20MB | <100ms | Untrusted/multi-tenant code |
| **microvm** | libkrun | TBD | TBD | Untrusted code, guest kernel boundary |

## 1. Bare Process (No Isolation)

```toml
[service.debug]
command = "./app"
isolation = "process"
```

Runs as a bare process with no isolation. All processes see the same `/proc`, environment, etc.

**When to use:**
- Trusted code only
- Debugging
- Same security boundary as the host

**Overhead:** None (bare metal speed)

## 2. Namespace Isolation (Default)

```toml
[service.api]
command = "./app"
isolation = "namespace"
```

Uses Linux namespaces (PID + Mount) to give each process its own `/proc` and isolated mount namespace. Environment variables are hidden between services.

**What's isolated:**
- `/proc` - Process tree hidden
- `/sys` - System interface hidden
- Mount namespace - Filesystem views separated

**What's shared:**
- Network (unless configured otherwise)
- System calls directly to kernel

**Overhead:** ~0 (kernel built-in since 2008)

**Startup:** <10ms

**Requirements:** Linux only

**When to use:**
- Multi-tenant deployments (trusted code)
- Microservices on one host
- You want isolation without performance cost
- **Default recommendation** for most users

### Example: Multi-tenant with Namespace Isolation

```toml
[service.api]
command = "uv run python app.py"
health = "/health"
isolation = "namespace"

[service.api.env]
DATABASE_PATH = "{data_dir}/{id}/app.db"
```

Each tenant process:
- Sees only its own `/proc`
- Can't spy on sibling processes
- Can't access sibling environment variables
- Runs at native speed

## 3. Sandbox Isolation (gVisor)

```toml
[service.untrusted]
command = "./user-plugin"
isolation = "sandbox"
```

Uses gVisor (runsc) to filter system calls. Untrusted code runs in a syscall sandbox.

**What's blocked:**
- Kernel module loading
- Raw socket access
- Dangerous syscalls (ptrace, etc.)
- Direct hardware access

**Overhead:** ~20MB memory per instance

**Startup:** <100ms (slightly slower, but cold-start)

**Requirements:**
- Linux
- gVisor installed (`apt install runsc` or similar)
- Compile with `--features sandbox`

**When to use:**
- User-supplied plugins/code
- Third-party integrations you don't trust
- Multi-tenant + untrusted code
- Compliance requirements

### Example: Sandbox for User Code

```toml
[service.api]
command = "./api"
isolation = "namespace"

[service.plugin]
command = "./user-plugin"
isolation = "sandbox"          # Untrusted
memory_limit_mb = 128          # Extra constrained
cpu_shares = 50                # Limited CPU
```

API runs in namespace isolation (trusted, fast). User plugins run in gVisor sandbox (untrusted, safe).

## 4. MicroVM Isolation (Future)

MicroVM isolation with libkrun. This adds a guest kernel boundary for hostile or unknown code while keeping Tenement's process-like spawn, route, health check, and hibernation model.

Planned for future releases.

The VMM is not trusted just because the guest runs behind a VM boundary. Tenement will run each libkrun VMM inside host-side namespaces, cgroups, UID/GID isolation, mount restrictions, and network policy. Guest and VMM should be treated as one security context because the VMM proxies host resources such as filesystem and networking access.

## Decision Flowchart

```
                  Start
                    │
                    ▼
        ┌───────────────────────┐
        │ Is the code trusted?  │
        │ (your own code, not   │
        │  user-uploaded)       │
        └──────────┬────────────┘
                   │
          ┌────────┴────────┐
          │                 │
        Yes                 No
          │                 │
          ▼                 ▼
    ┌───────────┐    ┌───────────────┐
    │ Need /proc│    │ Use SANDBOX   │
    │ isolation?│    │ (gVisor)      │
    └─────┬─────┘    └───────────────┘
          │
    ┌─────┴─────┐
    │           │
   Yes          No
    │           │
    ▼           ▼
┌──────────┐ ┌──────────┐
│NAMESPACE │ │ PROCESS  │
│ (default)│ │(no isol.)│
└──────────┘ └──────────┘
```

**Quick decision:**
- **Trusted code + multi-tenant** → `namespace` (default)
- **Trusted code + debugging** → `process`
- **Untrusted code needing syscall filtering** → `sandbox`
- **Untrusted code needing a guest kernel boundary** → `microvm`

## Choosing the Right Level

### Multi-tenant SaaS (Trusted Code)
→ **Use namespace isolation**
- Cheap, fast, good isolation
- Each tenant can't see others

```toml
[service.api]
isolation = "namespace"
```

### User-Supplied Code
→ **Use sandbox isolation**
- Extra security for untrusted code
- 20MB overhead is worth it

```toml
[service.user_code]
isolation = "sandbox"
memory_limit_mb = 256
cpu_shares = 100
```

### Hostile or Unknown Code
→ **Use microVM isolation** when available
- Tenant code gets its own guest kernel
- The libkrun VMM is still jailed by the host
- Best fit for code sandboxes, agent workspaces, and third-party execution

```toml
[service.user_code]
isolation = "microvm"
memory_limit_mb = 512
cpu_shares = 100
```

### Mixed Workload
→ **Use both**
- Trusted services: namespace
- Untrusted services: sandbox

```toml
[service.api]
isolation = "namespace"    # Your code

[service.user_plugins]
isolation = "sandbox"      # Their code
```

### Development/Debugging
→ **Use bare process**
- Easiest to debug
- Don't need isolation locally

```toml
[service.debug]
isolation = "process"
```

## Security Considerations

### Namespace Isolation

- **What it protects against:** Process inspection, environment snooping
- **What it doesn't protect:** OS-level exploits, kernel bugs
- **Best for:** Trusted code separation (multi-tenant with your own apps)

### Sandbox Isolation

- **What it protects against:** Most user-space exploits, kernel-facing attacks
- **What it doesn't protect:** Bugs in gVisor itself, hardware exploits
- **Best for:** Untrusted code, plugins, third-party services

### MicroVM Isolation

- **What it protects against:** Direct host-kernel syscall exposure from tenant code
- **What it doesn't protect:** Misconfigured VMM host access, broad virtio-fs mounts, broad network proxying
- **Best for:** Hostile or unknown code that warrants a guest kernel boundary

### Defense in Depth

Combine with resource limits:

```toml
[service.untrusted]
isolation = "sandbox"
memory_limit_mb = 128       # Can't eat all RAM
cpu_shares = 50             # Can't hog CPU
```

## Performance Comparison

Rough numbers on a modern Linux machine:

| Operation | Process | Namespace | Sandbox | Notes |
|-----------|---------|-----------|---------|-------|
| Spawn | 5ms | 8ms | 50ms | Sandbox is slower |
| First request | 1ms | 1ms | 2ms | Cold start penalty minimal |
| Request throughput | 100k/s | 100k/s | 50k/s | Sandbox adds ~50% overhead |
| Memory | 10MB | 10MB | 30MB | Sandbox adds ~20MB |

For most workloads, namespace isolation is the sweet spot: nearly native performance with good security.

## Next Steps

- [Configuration Reference](/guides/03-configuration) - Set isolation in config
- [Production Deployment](/guides/04-production) - Deploy with TLS and systemd
