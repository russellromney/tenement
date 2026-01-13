---
title: Limitations
description: What tenement can and cannot do
---

Understanding tenement's design constraints helps you decide if it's right for your use case.

## Hard Limitations

These are fundamental to tenement's design and won't change:

### Linux Only

tenement requires Linux for:
- Namespace isolation (`unshare` syscalls)
- Cgroup resource limits (cgroups v2)
- gVisor sandbox (Linux kernel interface)

**Workaround:** Develop on macOS/Windows using a Linux VM or container, deploy to Linux.

### Single Server

tenement manages processes on one machine. For multi-server deployments:
- Use **slum** for fleet orchestration
- Use Kubernetes, Nomad, or Fly.io for cluster management

### No Container Images

tenement runs processes, not containers. You can't:
- Pull images from Docker Hub
- Use OCI registries (GitHub Container Registry, etc.)
- Layer-cache builds

**Workaround:** Pre-install dependencies on the host using uv, bun, nix, or build static binaries.

### No Built-in Networking

tenement doesn't manage:
- Virtual networks between instances
- Service discovery
- Cross-instance DNS

**Workaround:** Instances communicate via localhost. Use environment variables for service URLs.

## Soft Limitations

These are current constraints that may change:

### No Log Persistence

Logs are kept in memory (ring buffer) and not persisted to disk. On restart, logs are lost.

**Workaround:** Ship logs to external service (Loki, CloudWatch, Papertrail).

### No Built-in Metrics Storage

Prometheus metrics are exported but not stored. You need an external Prometheus server.

**Workaround:** Run Prometheus + Grafana, or use a hosted metrics service.

### No Secrets Management

tenement doesn't encrypt or manage secrets. Environment variables are stored in plain text in config.

**Workaround:** Use external secrets managers (Vault, Doppler, AWS Secrets Manager) and inject at runtime.

### No Automatic Scaling

tenement doesn't auto-scale based on load. You manually spawn/stop instances.

**Workaround:** Write a script that monitors metrics and spawns/stops instances.

### No Rolling Updates

The `ten deploy` command spawns new instances but doesn't automatically stop old ones.

**Workaround:** Use `ten route` for blue-green deployments, manually stop old versions.

## Non-Goals

These features are explicitly out of scope:

### Container Ecosystem Compatibility

tenement won't:
- Support Docker images
- Integrate with Kubernetes
- Run OCI containers

**Alternative:** Use Docker, Podman, or containerd.

### Multi-Cloud Orchestration

tenement won't:
- Provision cloud resources
- Manage VMs across providers
- Handle cloud-specific networking

**Alternative:** Use Terraform, Pulumi, or cloud-native tools.

### Database Management

tenement won't:
- Run databases
- Manage database connections
- Handle migrations

**Alternative:** Use managed databases or run databases separately.

### Serverless Functions

tenement won't:
- Cold-start functions per request
- Run short-lived functions
- Manage function deployments

**Alternative:** Use Lambda, CloudFlare Workers, or Deno Deploy.

## Comparison Table

| Feature | tenement | Docker | Kubernetes |
|---------|----------|--------|------------|
| Single server | Yes | Yes | No (cluster) |
| Multi-server | No (use slum) | Swarm | Yes |
| Container images | No | Yes | Yes |
| Native performance | Yes | ~95% | ~90% |
| Setup complexity | Low | Medium | High |
| Memory overhead | ~0-20MB | ~50MB | ~200MB+ |
| Scale-to-zero | Built-in | Manual | Complex |

## When NOT to Use tenement

- **You need multi-server deployments** → Kubernetes, Nomad, Fly.io
- **You need container images** → Docker, Podman
- **You need Windows/macOS production** → Docker
- **You need serverless** → Lambda, CloudFlare Workers
- **You need managed databases** → Cloud providers
- **You need high availability** → Multiple servers with load balancer

## When to Use tenement

- **Multi-tenant SaaS on a budget** → tenement + scale-to-zero
- **Microservices on one server** → tenement + namespace isolation
- **Development environments** → tenement + fast iteration
- **Cost-sensitive deployments** → tenement + overstuff VPS
- **Simple deployments** → tenement + single binary

## Next Steps

- [Concepts](/intro/03-concepts) - Architecture and terminology
- [Quick Start](/intro/01-quick-start) - Get running in 5 minutes
- [Isolation Levels](/guides/01-isolation) - Security options
