# tenement

**Process hypervisor for single-server deployments.**

Pack 100+ services on a $5 VPS. Each customer gets their own process - spawn on demand, stop when idle, wake on first request.

```
customer1.app.com → api:customer1 → isolated process
customer2.app.com → api:customer2 → isolated process
```

Write single-tenant code. Deploy it everywhere.

## Install

```bash
cargo install tenement-cli
```

## Quick Start

```toml
# tenement.toml
[service.api]
command = "uv run python app.py"
health = "/health"
```

```bash
ten serve --port 8080 --domain localhost
ten spawn api --id prod

curl http://prod.api.localhost:8080/
```

Your app reads `PORT` from environment. tenement handles routing.

## What You Get

- **Subdomain routing** - `prod.api.example.com` → `api:prod`
- **Scale-to-zero** - Stop idle instances, wake on request
- **Process isolation** - Namespace separation, no container overhead
- **Weighted routing** - Blue-green and canary deployments
- **Auto-restart** - Health checks with exponential backoff
- **Built-in TLS** - Let's Encrypt certificates
- **One config file** - No YAML sprawl

## Who It's For

You want Fly Machines capabilities on your own server. You're running trusted code (your own apps, not arbitrary user code). You want to overstuff a VPS without Kubernetes complexity.

Pairs well with SQLite + WAL replication - write single-tenant software, give each customer their own database file.

## Documentation

**[tenement.dev](https://tenement.dev)** - Full docs, guides, and reference.

## Development

```bash
cargo test    # Run tests
cargo bench   # Run benchmarks
```

## License

Apache 2.0
