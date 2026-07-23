# Genome

A self-hosted Git forge, written in Rust — think GitHub/Forgejo/GitLab, plus
Coder-style browser dev workspaces, but API-first and API-only: every
capability (repos, issues, PRs, CI/CD, wikis, packages, dev workspaces...) is
reachable through a single GraphQL endpoint, the git smart-HTTP/SSH
transports, or a handful of REST routes for binary transfer. There is no
bundled web UI — bring your own frontend, or drive it entirely from scripts,
CI, and bots.

It embeds its own database (Hiqlite — Raft-replicated SQLite, no separate
service to run), speaks the GitHub Actions workflow YAML format for CI, and
runs CI/dev-workspace jobs as real Docker containers.

## Status

Built in a single autonomous session. The core is real and live-tested (not
just compiled): git clone/push over HTTP and SSH, GitHub Actions-compatible
CI running real Docker jobs, Coder-like containerized dev workspaces,
GraphQL API, package registry. See `JOURNAL.md` for the build log and an
honest list of what is and isn't production-hardened, and `DOC.md` for full
architecture/API/deployment reference.

## Quickstart

```bash
docker compose up -d --build
```

Or pull the published image instead of building locally:

```bash
docker run -p 8000:8000 -v ./data:/data ghcr.io/athenox14/genome:latest
```

See [Releases](https://github.com/Athenox14/Genome/releases) for tagged
versions — each release is built and published to `ghcr.io/athenox14/genome`
automatically by CI.

Then, end to end:

```bash
curl -s localhost:8000/graphql -H 'Content-Type: application/json' -d '{
  "query": "mutation { register(username: \"alice\", email: \"a@example.com\", password: \"correct-horse-battery\") { id } }"
}'

TOKEN=$(curl -s localhost:8000/graphql -H 'Content-Type: application/json' -d '{
  "query": "mutation { login(username: \"alice\", password: \"correct-horse-battery\") { token } }"
}' | jq -r .data.login.token)

curl -s localhost:8000/graphql -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' -d '{
  "query": "mutation { createAccessToken(name: \"ci\") { token } }"
}'
# use the returned token as: -H "Authorization: token genome_pat_..."
```

For pure automation deployments, set `ADMIN_BOOTSTRAP_TOKEN` instead —
it provisions an admin user and a matching access token on first startup,
so no `register`/`login` round-trip is ever needed.

## Architecture, at a glance

One Rust crate per concern — git storage/transport, SSH, auth, CI executor,
dev workspaces, webhooks, the GraphQL schema, and the `server` binary that
wires them all into one axum process (plus an optional standalone `runner`
binary for offloading CI/workspace execution to separate machines).

Full breakdown, environment variables, deployment options, the complete
GraphQL mutation/query reference, and known limitations: **see [`DOC.md`](DOC.md)**.

## Development

```bash
cargo check --workspace
cargo test --workspace
cargo run -p xtask -- gen-docs   # regenerate DOC.md's schema/config/crate tables
```

CI (`.github/workflows/docker-publish.yml`) runs the same checks on every
push/PR, builds and pushes the Docker image to GHCR on pushes to `main` and
on `v*` tags, cuts a GitHub release for tags, and auto-commits `DOC.md`
whenever the schema/config drifts on a push to `main`.

## License

AGPL-3.0-or-later (see `Cargo.toml`).
