# Hello World

The simplest possible tenement example - a bash script HTTP server.

## Setup

```bash
chmod +x server.sh
```

## Run

```bash
# Start tenement
ten serve --port 8080 --domain localhost

# In another terminal, spawn the instance
ten spawn hello --id world

# Test it
curl http://world.hello.localhost:8080/
# Output: Hello, World!
```

## What's Happening

1. `ten serve` starts the tenement server on port 8080
2. `ten spawn hello --id world` starts `server.sh` with PORT env var
3. Requests to `world.hello.localhost:8080` route to the instance
