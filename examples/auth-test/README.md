# auth-test

Demonstrates that tenement passes all request headers through unchanged. Each tenant's app has its own bearer token auth, and tenement doesn't interfere.

## Run

```bash
ten serve --port 9090 --domain localhost
ten token-gen
ten spawn notes:alice
ten spawn notes:bob

# Get each tenant's token
curl http://alice.notes.localhost:9090/token
curl http://bob.notes.localhost:9090/token

# Alice's token works on alice, rejected by bob
curl -H "Authorization: Bearer <alice-token>" http://alice.notes.localhost:9090/notes  # 200
curl -H "Authorization: Bearer <alice-token>" http://bob.notes.localhost:9090/notes    # 403
```

The app handles auth. tenement just forwards bytes.
