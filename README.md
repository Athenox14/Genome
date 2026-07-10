# Genome

Self-hosted Git forge — an alternative to GitHub/Forgejo/GitLab plus Coder-style
browser dev workspaces, written in Rust. Built API-first: every feature is
reachable through the GraphQL API (or the git smart-HTTP/SSH transports, or a
handful of REST routes for binary transfer), so it can run headless with no
web UI at all. A Nuxt/Vue frontend is included for testing/browsing but is not
required in production.

## Status

This was built in a single autonomous session. The core is real and
live-tested (not just compiled): git clone/push over HTTP and SSH, GitHub
Actions-compatible CI running real Docker jobs, Coder-like containerized dev
workspaces, GraphQL API, package registry. See `JOURNAL.md` for the full
build log and honest list of what is and isn't production-hardened.

**Known gaps** (see `JOURNAL.md` for detail): no artifact-of-repo-mirroring for
existing repos' pre-receive hooks (only newly-created repos get the force-push
protection hook), no code-search/full-text search (only ILIKE-ish substring
match), no rename-repo-on-disk.

## Architecture

Rust workspace, one crate per concern:

| Crate | Responsibility |
|---|---|
| `entity` | SeaORM entity models for all tables |
| `migration` | SeaORM migrations |
| `git-core` | Bare repo storage, smart-HTTP transport, wiki, merge (merge/squash/rebase), branch protection ancestry checks |
| `ssh-server` | git-over-SSH (russh), pubkey auth against registered SSH keys |
| `auth` | JWT, PAT hashing, argon2 password hashing, TOTP 2FA, permission model |
| `actions` | GitHub-Actions-compatible workflow YAML parser + Docker-based job executor, encrypted secrets |
| `dev-env` | Coder-like containerized dev workspaces (bollard/Docker), auto-stop scheduling |
| `webhooks` | HMAC-signed webhook dispatch |
| `graphql-api` | The GraphQL schema (async-graphql) wiring everything together |
| `server` | axum binary: HTTP router (GraphQL, git smart-HTTP, packages, artifacts), spawns the SSH server and background loops (mirror sync, workspace auto-stop) |

`frontend/` is an independent Nuxt 3 + Vue 3 app for browsing/testing against
the GraphQL API — not required for production use.

## Running locally

Requires Docker (for Postgres + spawning Actions/workspace containers) and a
Rust toolchain.

```bash
docker compose up -d db
export DATABASE_URL="postgres://genome:genome@127.0.0.1:5432/genome"
export JWT_SECRET="change-me"
export REPOS_ROOT_PATH="./data/repos"
export SECRETS_ENCRYPTION_KEY="$(openssl rand -base64 32)"
cargo run -p server
```

The server runs migrations automatically on startup. See
`config/genome.example.toml` for the full list of environment variables
(`LISTEN_ADDR`, `SSH_LISTEN_ADDR`, `PACKAGES_ROOT_PATH`, `FRONTEND_URL`,
`DOCKER_SOCKET_PATH`, ...).

To also run the frontend: `cd frontend && npm install && npm run dev`
(defaults to pointing at `http://localhost:8000`).

## Production deployment

```bash
docker compose up -d --build
```

This builds `Dockerfile` (multi-stage Rust release build → slim Debian
runtime) and starts `db` + `server` (+ `frontend` if you want the web UI).
The image is also published to `ghcr.io/<owner>/genome` automatically by
`.github/workflows/docker-publish.yml` on every push to `main`.

Since this is meant to run **API-only** in production, the frontend service
in `docker-compose.yml` is optional — omit it (`docker compose up -d db
server`) if you only need the API/git/CI surface.

## API-first usage

Every user-facing action is a GraphQL mutation/query at `POST /graphql`.
Authenticate with either:
- `Authorization: Bearer <jwt>` — from the `login` mutation (supports TOTP via
  an optional `totpCode` argument).
- `Authorization: token <pat>` — from a personal access token created via
  `createAccessToken` (this is the intended path for pure API/automation use,
  since it doesn't require a login session).

Register a user and get a token, end to end:

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

### Mutation reference (partial — see `crates/graphql-api/src/mutation.rs` for exact args)

Auth & account: `register`, `login`, `addSshKey`, `removeSshKey`,
`enableTwoFactor`, `confirmTwoFactor`, `disableTwoFactor`,
`createAccessToken`, `revokeAccessToken`.

Repos: `createRepository`, `updateRepository`, `deleteRepository`,
`forkRepository`, `setRepoMirror`, `setRepoSecret`, `addCollaborator`,
`removeCollaborator`, `createWebhook`, `updateWebhook`, `deleteWebhook`,
`createBranchProtectionRule`.

Issues/PRs: `createIssue`, `updateIssue`, `commentOnIssue`,
`createPullRequest`, `mergePullRequest` (`mergeMethod`: merge/squash/rebase),
`closePullRequest`, `submitPullRequestReview`, `addReviewComment`,
`createLabel`, `deleteLabel`, `addLabelToIssue`, `removeLabelFromIssue`,
`createMilestone`, `deleteMilestone`, `setIssueMilestone`, `createProject`,
`addProjectColumn`, `addCardToColumn`.

Wiki: `writeWikiPage` (also `repository.wikiPages`/`wikiPage` queries).

Orgs: `createOrganization`, `addOrgMember`, `updateOrgMemberRole`,
`removeOrgMember`, `deleteOrganization`.

CI/CD: `triggerWorkflowDispatch` (workflows also auto-trigger on push if
`.github/workflows/*.yml` matches the pushed branch — this is what makes it
GitHub Actions-compatible: `run:` steps and `actions/checkout` work,
`uses:` for anything else is a no-op with a logged warning; use
`genome/upload-artifact` with `with: { name, path }` to publish a build
artifact, downloadable via `GET /artifacts/:id/download`).

Dev workspaces: `createDevWorkspace` (optional `autoStopMinutes`),
`startDevWorkspace`, `stopDevWorkspace`, `deleteDevWorkspace`,
`execInDevWorkspace`.

Admin: `adminListUsers`, `adminSetUserAdmin`, `adminDeactivateUser`,
`adminAllDevWorkspaces`.

### Non-GraphQL surfaces

- `git clone/push http://<host>/<owner>/<repo>.git` (Basic auth with a PAT as
  the password, or public repos need no auth).
- `git clone/push ssh://git@<host>:<ssh_port>/<owner>/<repo>.git` after
  registering a key via `addSshKey`.
- `GET/PUT /packages/:owner/:name/:version` — package registry.
- `GET /artifacts/:id/download` — CI artifact download.
- `GET /health` — liveness probe.

## Testing

```bash
cargo check --workspace
cargo test --workspace
cd frontend && npm run build
```

CI (`.github/workflows/docker-publish.yml`) runs the same checks on every
push/PR, plus the Docker build+push to GHCR on pushes to `main`.
