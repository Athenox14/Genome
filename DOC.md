# Genome — Full Platform Documentation

This is the complete reference for using Genome: architecture, deployment,
authentication, the full GraphQL API surface, git transports, CI/CD
(Actions), dev workspaces, wiki, packages, webhooks, and admin operations.
For the shorter quick-start, see `README.md`. For the build history and
known limitations, see `JOURNAL.md`.

---

## 1. Architecture

Rust workspace, one crate per concern, all wired together by `crates/server`:

| Crate | Responsibility |
|---|---|
| `entity` | Plain row structs for every table, queried via raw SQL through `hiqlite::Client` |
| `migration` | Hiqlite (embedded, Raft-replicated SQLite) schema migrations |
| `git-core` | Bare repo storage, git smart-HTTP transport, wiki (second bare repo per project), merge (merge/squash/rebase), branch-protection ancestry checks, pre-receive hook generation |
| `ssh-server` | git-over-SSH (russh), pubkey auth against registered SSH keys |
| `auth` | JWT, PAT hashing, argon2 password hashing, permission model |
| `actions` | GitHub-Actions-compatible workflow YAML parser + Docker-based job executor, AES-256-GCM-encrypted secrets |
| `dev-env` | Coder-like containerized dev workspaces (bollard/Docker), auto-stop scheduling, HTTP reverse-proxy to the workspace |
| `webhooks` | HMAC-SHA256-signed webhook dispatch |
| `graphql-api` | The GraphQL schema (async-graphql) wiring everything above together |
| `server` | axum binary: HTTP router (GraphQL, git smart-HTTP, packages, artifacts, OAuth-free bootstrap), embeds the Hiqlite database node, spawns the SSH server, runs background loops (mirror sync, workspace auto-stop) |
| `runner` | Optional standalone binary: polls the main server for queued CI jobs (`/runner/claim`) and executes them, for scaling job execution to separate machines/pods without deploying the full API stack there |

`frontend/` is an independent Nuxt 3 + Vue 3 app for browsing/testing
against the GraphQL API. **Not required in production** — Genome is
API-first; every capability is reachable via GraphQL, git transports, or a
handful of REST routes, so it runs headless with zero UI.

### Database

Hiqlite (https://github.com/sebadob/hiqlite) — an embeddable, Raft-replicated
SQLite database — runs **inside the `server` process itself**. There is no
separate database service to provision, patch, back up, or fail over
independently. A single-node deployment is the default and simplest case;
multi-node configuration (real HA, automatic leader failover) is possible by
listing multiple `hiqlite::Node` entries in `NodeConfig` (see
`crates/server/src/main.rs`) — not yet wired to an env-var-driven multi-node
config out of the box, but the architecture supports it without further
rewrites.

---

## 2. Deployment

### Local development

```bash
export JWT_SECRET="change-me"
export REPOS_ROOT_PATH="./data/repos"
export DATA_DIR="./data/hiqlite"
export SECRETS_ENCRYPTION_KEY="$(openssl rand -base64 32)"
export ADMIN_BOOTSTRAP_TOKEN="$(openssl rand -hex 32)"   # see §3
cargo run -p server
```

Migrations run automatically on every startup (idempotent). See
`config/genome.example.toml` for every environment variable.

### Production

```bash
docker compose up -d --build
```

Builds the multi-stage `Dockerfile` (Rust release build → slim Debian
runtime) and starts `server` (+ optionally `frontend`, `nora` package-cache
sidecar — both are commented-out/optional services in `docker-compose.yml`).
The image is also published to `ghcr.io/<owner>/genome` on every push to
`main` via `.github/workflows/docker-publish.yml`.

Since this is meant to run **API-only**, omit `frontend`:
`docker compose up -d server`.

### Environment variables reference

| Variable | Default | Purpose |
|---|---|---|
| `DATA_DIR` | `./data/hiqlite` | Embedded database storage |
| `JWT_SECRET` | *(required)* | Signs JWTs; also seeds the Hiqlite Raft/API secrets and the at-rest encryption key |
| `REPOS_ROOT_PATH` | `./repos` | Bare git repo storage root |
| `LISTEN_ADDR` | `0.0.0.0:8000` | HTTP (GraphQL + git smart-HTTP + REST) bind address |
| `SSH_LISTEN_ADDR` | `0.0.0.0:2222` | git-over-SSH bind address |
| `SSH_HOST_KEY_PATH` | `./data/ssh_host_key` | Persisted SSH host key (generated on first run) |
| `HIQLITE_API_ADDR` | `127.0.0.1:8200` | Embedded DB node's internal API address |
| `HIQLITE_RAFT_ADDR` | `127.0.0.1:8100` | Embedded DB node's internal Raft address |
| `DOCKER_SOCKET_PATH` | platform default | Docker socket for Actions job execution + dev workspaces |
| `SECRETS_ENCRYPTION_KEY` | *(unset)* | 32-byte base64 key for encrypting Actions secrets (`setRepoSecret`); required for that feature only |
| `PACKAGES_ROOT_PATH` | `{repos_root_path}/../packages` | Package registry storage |
| `FRONTEND_URL` | `http://localhost:3000` | Used for any redirect targets referencing the frontend |
| `ADMIN_BOOTSTRAP_TOKEN` | *(unset)* | See §3 — pure-API bootstrap credential |

---

## 3. Authentication

Three ways to authenticate, all via the `Authorization` header:

1. **`Authorization: Bearer <jwt>`** — from the `login` mutation.
2. **`Authorization: token <pat>`** — a personal access token from
   `createAccessToken`. **This is the intended path for API/automation use**,
   since it needs no session/login step per call.
3. **Bootstrap admin token** (`ADMIN_BOOTSTRAP_TOKEN` env var) — if set, an
   admin user + a PAT hashing to this exact value are provisioned on first
   startup (idempotent). `Authorization: token <ADMIN_BOOTSTRAP_TOKEN>`
   authenticates as admin from the very first request — **no `register` or
   `login` mutation is ever required** for a pure-API deployment.

`register`/`login` remain available (useful for a human admin bootstrapping
via the frontend, or multi-user setups where new accounts self-register),
but nothing in the platform *requires* them if `ADMIN_BOOTSTRAP_TOKEN` is used.

### End-to-end example (pure API, no login)

```bash
curl -s localhost:8000/graphql -H "Authorization: token $ADMIN_BOOTSTRAP_TOKEN" \
  -H 'Content-Type: application/json' -d '{
    "query": "mutation { createRepository(name: \"myrepo\", description: \"\", isPrivate: false) { id name } }"
}'
```

---

## 4. GraphQL API — full mutation/query reference

All requests: `POST /graphql` (a `GET /graphql` GraphiQL playground is also
served for interactive exploration). Exact argument types/names are in
`crates/graphql-api/src/mutation.rs` / `query.rs` / `types.rs`.

### Queries

`me`, `user(username)`, `mySshKeys`, `myRepositories`,
`repository(owner, name)`, `organizations`, `organization(name)`,
`devWorkspaces`, `adminAllDevWorkspaces`, `myAccessTokens`,
`adminListUsers(limit, offset)`, `myNotifications(unreadOnly)`,
`myActivity(limit)`, `myPackages`, `search(query)`.

### `repository` nested fields (all resolved on the `RepositoryObject` type)

`ownerLogin`, `issues`, `pullRequests`, `workflowRuns`, `packages`,
`secretNames`, `branches`, `tree(ref, path)`, `wikiPages`, `wikiPage(page)`,
`commits(ref, limit)`, `activity(limit)`, `collaborators`, `labels`,
`milestones`, `projects` (with nested `columns` → `cards` → `issue`),
`branchProtectionRules`, `webhooks`.

### Mutations, grouped

**Auth & account**: `register`, `login`, `addSshKey`, `removeSshKey`,
`createAccessToken`, `revokeAccessToken`.

**Repositories**: `createRepository`, `updateRepository`,
`deleteRepository`, `forkRepository`, `setRepoMirror`, `setRepoSecret`,
`addCollaborator`, `removeCollaborator`, `createWebhook`, `updateWebhook`,
`deleteWebhook`, `createBranchProtectionRule`.

**Issues & pull requests**: `createIssue`, `updateIssue`, `commentOnIssue`,
`createPullRequest`, `mergePullRequest` (`mergeMethod`:
`merge`/`squash`/`rebase`), `closePullRequest`, `submitPullRequestReview`,
`addReviewComment`, `createLabel`, `deleteLabel`, `addLabelToIssue`,
`removeLabelFromIssue`, `createMilestone`, `deleteMilestone`,
`setIssueMilestone`, `createProject`, `addProjectColumn`, `addCardToColumn`.

**Wiki**: `writeWikiPage(repoId, page, content, message)`.

**Organizations**: `createOrganization`, `addOrgMember`,
`updateOrgMemberRole`, `removeOrgMember`, `deleteOrganization`.

**CI/CD**: `triggerWorkflowDispatch`.

**Dev workspaces**: `createDevWorkspace` (optional `autoStopMinutes`),
`startDevWorkspace`, `stopDevWorkspace`, `deleteDevWorkspace`,
`execInDevWorkspace`.

**Notifications**: `markNotificationRead`.

**Admin**: `adminSetUserAdmin`, `adminDeactivateUser`.

---

## 5. Git access

- **HTTP**: `git clone`/`push http://<host>/<owner>/<repo>.git` — Basic auth
  with a PAT as the password (public repos need no auth for read).
- **SSH**: `git clone`/`push ssh://git@<host>:<ssh_port>/<owner>/<repo>.git`
  after registering a public key via `addSshKey`. Auth is by fingerprint
  match against registered keys.
- **Branch protection**: `createBranchProtectionRule` sets a required-reviews
  count and/or `blockForcePush`. Force-push blocking is enforced by a real
  git `pre-receive` hook (written into every newly-created repo) — it
  rejects the push at the git protocol level, not just after the fact.
  Required-reviews is enforced in `mergePullRequest`.
- **Merge methods**: `merge` (2-parent merge commit), `squash` (single
  commit atop target), `rebase` (replays source commits onto target).
- **Wiki**: every repo gets a second bare repo (`{name}.wiki.git`)
  automatically; pages are plain Markdown files, one commit per
  `writeWikiPage` call.

---

## 6. CI/CD (Actions)

Workflows are discovered from `.github/workflows/*.yml`/`.yaml` in the
pushed tree — **this is what makes it GitHub-Actions-compatible**: existing
GH Actions YAML mostly just works.

Supported: `on: push`/`pull_request`/`workflow_dispatch` (string, list, or
map-with-filters form), `jobs.<id>.needs` (bare string or list — both
accepted), `runs-on` (mapped to a Docker image; `ubuntu-latest` → a plain
Ubuntu image, anything that looks like an image tag is used directly),
`steps[].run` (executed via `sh -c` inside the job's container),
`steps[].uses: actions/checkout@*` (no-op — the repo tree is already present
in the container). `env`, per-step `env`.

**Not supported**: the GitHub Actions marketplace. Any `uses:` other than
`actions/checkout` is logged and skipped (`... not supported in this runner,
skipping`) rather than failing the job — so a real-world workflow with
marketplace actions will still run its `run:` steps, just without whatever
that action would have set up. There is no full `ubuntu-latest`-equivalent
image with GitHub's huge pre-installed toolset — the default image is a
plain `ubuntu:22.04`/similar, so commands like `sudo`, `npm`, language
toolchains etc. are **not** pre-installed unless your workflow installs them
itself or you point `runs-on` at an image that already has them.

**Secrets**: `setRepoSecret(repoId, name, value)` stores an
AES-256-GCM-encrypted value (`SECRETS_ENCRYPTION_KEY` required). Job steps
can reference `${{ secrets.NAME }}` and it's substituted before execution.
Secret values are never readable back via the API (`repository.secretNames`
lists names only).

**Artifacts**: a step with `uses: genome/upload-artifact` and
`with: { name, path }` copies that path out of the finished container to
disk; download via `GET /artifacts/:id/download` (permission-checked
against the owning repo).

**Triggering**: workflows auto-trigger on a matching push, or on-demand via
`triggerWorkflowDispatch`. Execution happens either in-process (the `server`
binary runs the job directly against its Docker socket) or, if a standalone
`crates/runner` is connected, that runner claims and executes it instead —
never both (in-process runs are recorded in the job history with a status
that a runner can never pick up, avoiding double execution).

### Standalone runner

```bash
export GENOME_SERVER_URL="http://genome-server:8000"
export GENOME_RUNNER_TOKEN="<a PAT from createAccessToken>"
export DOCKER_SOCKET_PATH="/var/run/docker.sock"
cargo run -p runner   # or the `runner` binary in the release image
```

Polls `POST /runner/claim` every few seconds; on a claimed job, executes it
via the same `actions::Executor` logic as in-process execution, then
reports back via `POST /runner/jobs/:id/complete`. Lets CI execution scale
out to separate machines/pods without deploying the full GraphQL/git-hosting
stack on them. Dev-workspace hosting via the runner (as opposed to CI jobs)
is reserved in the schema (`kind = 'dev_workspace_action'`) but not yet wired
to an active polling loop — currently dev workspaces are only managed
in-process by `server`.

---

## 7. Dev workspaces (Coder-like)

`createDevWorkspace(name, template?, image?, autoStopMinutes?)` starts a
Docker container with resource limits, optionally cloning a repo into it on
start. Pass `template` to pick a built-in configuration (`"code-server"`,
`"rust-dev"`, `"node-dev"` — see `dev_env::templates`), which resolves to an
image plus a set of named ports; or pass a raw `image` for advanced/custom
use, which falls back to the single-port code-server-only behavior. Access a
workspace's default port through Genome's own domain via
`GET /workspaces/:id/proxy/*path`, or an explicitly named port via
`GET /workspaces/:id/proxy_port/:portName/*path` (both reverse-proxied to
the container) rather than exposing raw container ports. `autoStopMinutes` +
a background loop stop idle workspaces automatically; `execInDevWorkspace`
runs an arbitrary command inside a running workspace.

---

## 8. Package registry

Minimal generic registry: `PUT`/`GET /packages/:owner/:name/:version`
(immutable once published — re-publishing the same version is rejected).
`myPackages`/`repository.packages` list metadata (not content).

For real multi-ecosystem package caching (npm, Cargo, PyPI, Maven, Docker,
etc.) as a CI speedup, an optional `nora` sidecar
(`getnora/nora:latest`, see `docker-compose.yml` and
`config/nora.example.toml`) proxies+caches upstream registries with a 7-day
unused-cache eviction rule. It runs alongside Genome (not embedded in the
same process — its published crate has no reusable library surface).

---

## 9. Webhooks

`createWebhook(repoId, targetUrl, secret, events)` registers a target for
`issues`, `issue_comment`, `pull_request`, and `push` events. Deliveries are
POSTed with `X-Genome-Event: <event>` and
`X-Genome-Signature-256: sha256=<hmac>` (HMAC-SHA256 of the raw JSON body,
keyed with the webhook's `secret`) — verify this to authenticate the
delivery as genuinely from your Genome instance. Delivery failures are
logged and never fail the triggering mutation.

---

## 10. OAuth2 — *not currently included*

An OAuth2 identity-provider feature (Genome issuing tokens to third-party
apps) was built and then removed on request, since it's for delegated
third-party-app auth rather than direct API automation, which PATs already
cover. If ever needed again it can be re-added; see `JOURNAL.md` for the
removal note and the git history around that change.

---

## 11. Admin operations

`adminListUsers(limit, offset)`, `adminSetUserAdmin(userId, isAdmin)`,
`adminDeactivateUser(userId)`, `adminAllDevWorkspaces` — all gated on
`claims.is_admin`, returning a `forbidden` GraphQL error otherwise. A
deactivated user's login (JWT or PAT) is rejected immediately.

---

## 12. Testing & CI

```bash
cargo check --workspace
cargo test --workspace         # includes DB-backed integration tests --
                                # each spins up its own ephemeral single-node
                                # Hiqlite instance, no external service needed
cd frontend && npm run build
```

`.github/workflows/docker-publish.yml` runs the same checks on every
push/PR, plus builds+pushes the Docker image to GHCR on pushes to `main`.

---

## 13. Binary sizes (release profile: opt-level=3, lto, codegen-units=1, strip)

- `server`: 37.4 MB (was 50.8 MB before this profile)
- `runner`: 5.2 MB

## 14. Known limitations

See `JOURNAL.md` for the full, honest build log, but the headline gaps:

- No full-text/code search (substring `LIKE` match only).
- No repo rename-on-disk.
- Existing repos created before branch-protection support don't get the
  pre-receive hook backfilled automatically (only newly-created repos do).
- GitHub Actions marketplace `uses:` actions are skipped, not executed
  (only `actions/checkout` and plain `run:` steps work).
- Standalone runner support for dev-workspace hosting (as opposed to CI
  jobs) is schema-reserved but not implemented.
- Multi-node Hiqlite (true multi-machine HA) is architecturally supported
  but not yet exposed via a ready-made multi-node env-var configuration —
  single-node is the tested, default path.
