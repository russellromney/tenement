# Multi-Environment Example Makefile

.PHONY: build run clean test deploy-v2 rollback

# Build all compiled apps
build:
	@echo "Building Go worker..."
	cd apps/go && go build -o go-worker
	@echo "Building Rust cache..."
	cd apps/rust && cargo build --release
	@echo "Build complete!"

# Start tenement server
run: build
	ten serve --port 8080 --domain localhost

# Clean build artifacts and runtime data
clean:
	rm -f apps/go/go-worker
	rm -rf apps/rust/target
	rm -rf data/
	rm -f /tmp/tenement/*.sock

# Test all endpoints (requires server running)
test:
	@echo "Testing API (prod)..."
	@curl -s http://prod.api.localhost:8080/ | jq .
	@echo "\nTesting API (staging)..."
	@curl -s http://staging.api.localhost:8080/ | jq .
	@echo "\nTesting Web (weighted)..."
	@curl -s http://web.localhost:8080/ | jq .
	@echo "\nTesting Worker (prod)..."
	@curl -s http://prod.worker.localhost:8080/ | jq .
	@echo "\nTesting Cache (weighted)..."
	@curl -s http://cache.localhost:8080/ | jq .
	@echo "\nAll tests passed!"

# Demo: Blue/Green deployment
deploy-v2:
	@echo "Deploying api:v2..."
	ten deploy api --version v2 --weight 0
	@echo "\nTesting v2..."
	curl -s http://v2.api.localhost:8080/health
	@echo "\n\nSwapping traffic..."
	ten route api --from prod --to v2
	@echo "\nDone! Traffic now goes to v2"

# Demo: Rollback to prod
rollback:
	@echo "Rolling back to prod..."
	ten route api --from v2 --to prod
	ten stop api:v2 2>/dev/null || true
	@echo "Done!"

# Show help
help:
	@echo "Available targets:"
	@echo "  make build     - Compile Go and Rust apps"
	@echo "  make run       - Start tenement server"
	@echo "  make test      - Test all endpoints"
	@echo "  make clean     - Remove build artifacts"
	@echo "  make deploy-v2 - Demo blue/green deployment"
	@echo "  make rollback  - Rollback to prod"
