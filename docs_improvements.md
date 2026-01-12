Based on my analysis of both documentation systems, here's what I found:

The Problem
Tenement has TWO separate doc systems that are redundant and confusing:

tenement/docs - Old MkDocs setup (3 files, material theme)
tenement/website - Newer Astro/Starlight setup (8 files, better structure)
The MkDocs version is sitting unused while the website has better content. This creates confusion and means maintenance is split.

What Redlite Does Right
Single, clean system - Astro/Starlight only

Clear navigation structure with proper sections:

Getting Started (Introduction → Installation → Quick Start progression)
Usage (practical modes: embedded & server)
Commands (auto-generated)
Reference (deep dives)
Excellent content quality:

Includes "When NOT to use" sections (so important!)
Multi-language examples (Rust, Python, Node.js, CLI)
Progressive disclosure: simple → advanced
Shows both happy path and advanced patterns
Professional polish:

Proper frontmatter with descriptions
Consistent internal linking
Real working code (copy-paste ready)
Recommendations for Tenement
Critical:

Delete tenement/docs and mkdocs.yml - Keep website only
Fix broken links - Quick start references /guides/getting-started that don't exist
Flesh out all guides - Currently feel incomplete:
guides/01-isolation.md - needs content on namespace vs sandbox
guides/02-fleet.md - incomplete or missing
Important:
4. Reorganize sidebar to match redlite's clarity:


Getting Started
├── Introduction
├── Installation  
├── Quick Start
├── The Economics

Core Concepts
├── Isolation (namespace vs sandbox)
├── Fleet Mode (multi-server)

Use Cases
├── Multi-tenant SaaS
├── Scale-to-Zero

Reference
├── Configuration
├── Roadmap
Add "When NOT to use" sections - e.g., when Kubernetes is better, when systemd is fine

Complete all guides with working examples - Tenement is lower-level than Redlite, so include:

Nginx/Caddy routing configs
Health check endpoint examples (Go, Node, Python)
Cgroup limits explained
Migration guides from Docker/systemd
Add troubleshooting section - Common issues, how to debug

Nice to have:
8. Move economics content into Getting Started progression (currently isolated in guides)
9. Add diagram showing how isolation levels work
10. Add performance comparison table vs Docker/systemd

The website structure is already better than docs, so just consolidate and flesh it out!

