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

There is no bundled frontend — Genome is API-first and API-only; every
capability is reachable via GraphQL, git transports, or a handful of REST
routes, so it runs headless with zero UI.

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
runtime) and starts `server` (+ optionally the `nora` package-cache sidecar,
an optional service in `docker-compose.yml`). The image is also published to
`ghcr.io/<owner>/genome` on every push to `main` and on `v*` tags via
`.github/workflows/docker-publish.yml`, with a GitHub release cut for each
tag.

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
| `DOCKER_SOCKET_PATH` | platform default | Docker (or Docker-API-compatible) socket for Actions job execution + dev workspaces — see §2a |
| `SECRETS_ENCRYPTION_KEY` | *(unset)* | 32-byte base64 key for encrypting Actions secrets (`setRepoSecret`); required for that feature only |
| `PACKAGES_ROOT_PATH` | `{repos_root_path}/../packages` | Package registry storage |
| `FRONTEND_URL` | `http://localhost:3000` | Base URL used in any links/redirects that point back to a UI, if you run your own frontend against the API |
| `ADMIN_BOOTSTRAP_TOKEN` | *(unset)* | See §3 — pure-API bootstrap credential |

### 2a. Container isolation (avoiding a privileged host Docker socket)

CI job execution (`actions::Executor`) and dev workspaces (`dev_env::WorkspaceManager`)
both work by talking to a Docker Engine API over a socket — the shipped
`docker-compose.yml` does this by bind-mounting the **host's own**
`/var/run/docker.sock` into the `server` container ("Docker outside of
Docker", not Docker-in-Docker/DinD — there's no nested `dockerd` — but the
practical risk is the same shape: anything that can reach that socket has
root-equivalent control of the *host*, since it can start a container with
`-v /:/host` and chroot into it). This is the simplest thing that works,
but it's the single biggest privilege-escalation surface in a default
deployment.

`DOCKER_SOCKET_PATH` is a real, wired-up override (not just accepted and
logged) for both `server` and the standalone `runner` binary — point it at
a socket with a smaller blast radius instead of the host's main daemon:

- **Rootless Podman** (recommended, no code changes needed — Podman's
  socket speaks the same Docker Engine API `bollard` already uses):
  ```bash
  systemctl --user enable --now podman.socket
  # DOCKER_SOCKET_PATH=/run/user/$(id -u)/podman/podman.sock
  ```
  Running as an unprivileged user, in its own user namespace, means a
  container escape lands in that user's namespace, not root on the host.
  This is the same technique Docker's own `rootless` mode and CI systems
  like GitLab increasingly default to instead of the historical
  `docker:dind` sidecar.
- **Sysbox** (`nestybox/sysbox`) if a workload genuinely needs to run its
  own nested Docker/Kubernetes (e.g. a CI job whose `run:` steps do
  `docker build`, which rootless Podman alone doesn't help with) — an
  alternative OCI runtime that gives a container real user-namespace
  isolation *and* lets it run Docker-in-Docker safely, without
  `--privileged` and without a host socket mount at all. Run the `server`/
  `runner` container itself with `--runtime=sysbox-runc` and it can host
  its own isolated `dockerd`, socket-mounted only to itself.
- A dedicated **rootful-but-isolated** `dockerd` (e.g. a sibling container
  or VM with nothing else on it) is the fallback if neither of the above
  fits — smaller blast radius than sharing the *host's* daemon, even
  though it isn't rootless.

None of this is wired up as a default, since a working zero-config
`docker compose up` needs *some* socket available — but every deployment
that isn't purely local/throwaway should point `DOCKER_SOCKET_PATH` at one
of the above rather than the host socket bind-mount in the shipped compose
file.

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
via your own UI, or multi-user setups where new accounts self-register), but
nothing in the platform *requires* them if `ADMIN_BOOTSTRAP_TOKEN` is used.

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
`myActivity(limit)`, `myPackages`, `search(query)` (real SQLite-FTS5
full-text search — see §4a — over repositories, issues, users, and indexed
file content, not substring `LIKE`).

### `repository` nested fields (all resolved on the `RepositoryObject` type)

`ownerLogin`, `issues`, `pullRequests`, `workflowRuns`, `packages`,
`secretNames`, `branches`, `tree(ref, path)`, `fileContent(ref, path)` (a
single file's content — `content` as UTF-8 text plus `contentBase64` always,
`isBinary`/`size`; returns `null` for a missing path/ref or a file over
10MiB), `wikiPages`, `wikiPage(page)`, `commits(ref, limit)`,
`activity(limit)`, `collaborators`, `labels`, `milestones`, `projects` (with
nested `columns` → `cards` → `issue`), `branchProtectionRules`, `webhooks`.

### `issue` nested fields (all resolved on the `IssueObject` type)

`labels`, `milestone`, `comments` (posted via `commentOnIssue`, oldest
first).

### Mutations, grouped

**Auth & account**: `register`, `login`, `addSshKey`, `removeSshKey`,
`createAccessToken`, `revokeAccessToken`.

**Repositories**: `createRepository`, `updateRepository`,
`renameRepository(repoId, newName)` (moves the bare repo + wiki dir on
disk and updates the DB `name`, with best-effort rollback on partial
failure), `deleteRepository`, `forkRepository`, `setRepoMirror`,
`setRepoSecret`, `addCollaborator`, `removeCollaborator`, `createWebhook`,
`updateWebhook`, `deleteWebhook`, `createBranchProtectionRule`.

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

**Dev workspaces**: `createDevWorkspace` (optional `autoStopMinutes`,
`onRunner` — see §7), `startDevWorkspace`, `stopDevWorkspace`,
`deleteDevWorkspace`, `execInDevWorkspace`.

**Notifications**: `markNotificationRead`.

**Admin**: `adminSetUserAdmin`, `adminDeactivateUser`,
`adminBackfillPreReceiveHooks` (writes the branch-protection pre-receive
hook onto every repo — including ones created before that feature
existed; see §5).

### Full schema (auto-generated)

The exact, always-current SDL — every type, field, and argument — is
generated straight from the schema by `cargo run -p xtask -- gen-docs`
(checked in CI so this block can't drift from the code).

<!-- GRAPHQL_SCHEMA:START (auto-generated by `cargo run -p xtask -- gen-docs`, do not edit by hand) -->

```graphql
placeholder — run `cargo run -p xtask -- gen-docs` to fill this in
```

<!-- GRAPHQL_SCHEMA:END -->

---

## 4a. Search

`search(query)` runs real SQLite FTS5 full-text search (via `MATCH`, with
BM25 relevance ordering), not the substring `LIKE` matching this used to
be limited to. Each plain alphanumeric word in `query` becomes an
implicit-prefix match (`word*`, so results appear as you finish typing a
term); anything else is treated as a literal quoted phrase. Terms are
ANDed together. Covers:

- **Repositories** (`name`/`description`) and **users** (`username`).
- **Issues** (`title`/`body`), scoped to repos visible to the caller.
- **Code**: file contents across the *default branch* of every repo
  visible to the caller, returned as `SearchResults.code` — each hit
  carries the matching `repository`, `path`, and an excerpt (`snippet`,
  FTS5-generated, with `[b]...[/b]` match markers). Indexing happens
  automatically after every push that moves the default branch (see
  `index_repo_code_on_push` in `crates/server`): the whole tree is
  re-walked and re-indexed (not diffed), so a force-push/history rewrite
  is handled correctly. Binary-looking files (a NUL byte anywhere in their
  content), files over 256KB, and repos with more than 2000 files hit an
  indexing cap and are partially indexed rather than skipped or slow.
  Non-default branches are not indexed.

---

## 5. Git access

- **HTTP**: `git clone`/`push http://<host>/<owner>/<repo>.git` — Basic auth
  with a PAT as the password (public repos need no auth for read).
- **SSH**: `git clone`/`push ssh://git@<host>:<ssh_port>/<owner>/<repo>.git`
  after registering a public key via `addSshKey`. Auth is by fingerprint
  match against registered keys.
- **Branch protection**: `createBranchProtectionRule` sets a required-reviews
  count and/or `blockForcePush`. Force-push blocking is enforced by a real
  git `pre-receive` hook, written automatically into every newly-created
  repo — it rejects the push at the git protocol level, not just after the
  fact. Required-reviews is enforced in `mergePullRequest`. Repos created
  *before* branch-protection support existed don't get this hook
  automatically; an admin can backfill it onto every repo at once (new
  ones included, harmlessly — the write is an unconditional overwrite, not
  conditional on "missing") via `adminBackfillPreReceiveHooks`.
- **Merge methods**: `merge` (2-parent merge commit), `squash` (single
  commit atop target), `rebase` (replays source commits onto target).
- **Rename**: `renameRepository(repoId, newName)` updates the DB `name`
  and moves both the bare repo directory and (if present) its wiki
  directory on disk to match, rejecting the rename if the owner already
  has another repo named `newName`. Best-effort rolls back any disk
  rename(s) already performed if a later step fails, so disk and DB don't
  end up disagreeing about the repo's name. A dev workspace already
  running against the old path (its bind-mount was resolved once at
  creation time) won't pick up the new path until recreated.
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

**Marketplace actions** (`crates/actions/src/marketplace.rs`): `uses:` steps
now do more than a no-op for `actions/checkout`:

- `uses: docker://image[:tag]` runs that image directly as a short-lived
  sibling container: the job's `/workspace` is copied in, the image's own
  entrypoint/cmd runs, and `/workspace` is copied back out afterward so
  later steps see any files the action wrote. `with:` values become
  `INPUT_*` env vars.
- `uses: owner/repo[/path]@ref` fetches that action from GitHub (a full
  clone, not a shallow one — see the module docs for why), resolves `ref`
  against tags/branches/raw SHAs, and reads its `action.yml`/`action.yaml`:
  - `runs.using: docker` — same container model as `docker://` above,
    building from a `Dockerfile` first if `runs.image` isn't already a
    `docker://` reference. `runs.entrypoint`/`runs.args` support
    `${{ inputs.NAME }}` substitution.
  - `runs.using: composite` — nested `steps` run against the *same* job
    container (like a real composite action). One level deep only: a
    composite action nested inside another composite action is logged and
    skipped rather than recursing.
  - `runs.using: node12/16/18/20` — JS actions actually run, inside a
    helper `node:<version>-slim` container (the job's own `runs-on` image
    has no reason to include Node) with the action's source copied in
    alongside the job's `/workspace`.
  - Anything else (an input-less docker action with no `image`, an
    unresolvable ref, a runtime we don't model, a fetch failure) is logged
    and skipped exactly like today's "not supported" path — the job
    continues, that step is a no-op.

There's still no full `ubuntu-latest`-equivalent image with GitHub's huge
pre-installed toolset — the default image is a plain `ubuntu:22.04`/
similar, so commands like `sudo`, `npm`, language toolchains etc. are
**not** pre-installed unless your workflow installs them itself, a
marketplace action sets them up, or you point `runs-on` at an image that
already has them.

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
stack on them.

The runner also claims and executes `dev_workspace_action` jobs
(`kind = 'dev_workspace_action'`) — see §7 — via its own local Docker
daemon (`dev_env::WorkspaceManager`), reporting results (e.g. the created
container's id) back in the same completion call's new `result` field.

---

## 7. Dev workspaces (Coder-like)

`createDevWorkspace(name, template?, image?, autoStopMinutes?, onRunner?)`
starts a Docker container with resource limits, optionally cloning a repo
into it on start. Pass `template` to pick a built-in configuration
(`"code-server"`, `"rust-dev"`, `"node-dev"` — see `dev_env::templates`),
which resolves to an image plus a set of named ports; or pass a raw
`image` for advanced/custom use, which falls back to the single-port
code-server-only behavior. Access a workspace's default port through
Genome's own domain via `GET /workspaces/:id/proxy/*path`, or an
explicitly named port via `GET /workspaces/:id/proxy_port/:portName/*path`
(both reverse-proxied to the container) rather than exposing raw container
ports. `autoStopMinutes` + a background loop stop idle workspaces
automatically; `execInDevWorkspace` runs an arbitrary command inside a
running workspace.

**Standalone-runner-hosted workspaces**: pass `onRunner: true` to have a
connected standalone `runner` (§6) create the container against *its own*
Docker daemon instead of `server`'s — useful for keeping dev-workspace
compute off the machine running the API/git-hosting stack. This enqueues a
`dev_workspace_action` job and waits (polling the DB, up to 20s) for a
runner to claim and execute it; the returned workspace has
`status: "pending_runner"` and `runnerId: null` until that happens, then
`status: "running"` with `runnerId` set. `deleteDevWorkspace` and
`execInDevWorkspace` also route through the runner for a workspace it
hosts. **Known gaps**: `startDevWorkspace`/`stopDevWorkspace` and the
auto-stop background loop are not implemented for runner-hosted workspaces
yet (they error/skip rather than acting on the wrong Docker daemon), and —
the bigger one — live port-proxying (`GET /workspaces/:id/proxy/*path`)
doesn't work for a runner-hosted workspace at all: its container lives on
the runner's Docker host, which `server` has no network path to reach or
tunnel through yet. A runner-hosted workspace is reachable via
`execInDevWorkspace` (e.g. to inspect it or run a headless task) but not
via the HTTP proxy today.

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
`adminDeactivateUser(userId)`, `adminAllDevWorkspaces`,
`adminBackfillPreReceiveHooks` (§5) — all gated on `claims.is_admin`,
returning a `forbidden` GraphQL error otherwise. A deactivated user's
login (JWT or PAT) is rejected immediately.

---

## 12. Testing & CI

```bash
cargo check --workspace
cargo test --workspace         # includes DB-backed integration tests --
                                # each spins up its own ephemeral single-node
                                # Hiqlite instance, no external service needed
cargo run -p xtask -- gen-docs --check   # DOC.md's schema section stays in sync
```

`.github/workflows/docker-publish.yml` runs the same checks on every
push/PR, builds+pushes the Docker image to GHCR on pushes to `main` and on
`v*` tags, and cuts a GitHub release for each `v*` tag.

---

## 13. Binary sizes (release profile: opt-level=3, lto, codegen-units=1, strip)

- `server`: 37.4 MB (was 50.8 MB before this profile)
- `runner`: 5.2 MB

## 14. Known limitations

See `JOURNAL.md` for the full, honest build log, but the headline gaps:

- GitHub Actions marketplace `uses:` support (§6) covers `docker://`
  images, `owner/repo[/path]@ref` actions with `runs.using: docker`
  (including building from a `Dockerfile`), one level of `composite`
  actions, and JS actions (`node12`/`16`/`18`/`20`) — but not a nested
  composite-inside-composite action, and not the actual GitHub Actions
  toolkit's finer behaviors (`core.setOutput`/step outputs, `GITHUB_ENV`/
  `GITHUB_PATH` files, caching, etc.). There is still no full
  `ubuntu-latest`-equivalent image with GitHub's huge pre-installed
  toolset.
- Standalone-runner-hosted dev workspaces (§7) support create/delete/exec,
  but not start/stop or live port-proxying to the workspace — the
  runner's container lives on a Docker daemon `server` has no network path
  to (no reverse tunnel exists between them yet).
- Code search (§4a) only indexes each repo's *default branch*; other
  branches, and git history, aren't searchable.
- Merge is a real 2-parent/squash/rebase commit, but there's still no
  rich diff-review UI, no LFS, and no SSH-transport for anything beyond
  git itself (e.g. no `git-lfs-transfer`).
- Multi-node Hiqlite (true multi-machine HA) is architecturally supported
  but not yet exposed via a ready-made multi-node env-var configuration —
  single-node is the tested, default path.
- CI/dev-workspace execution talks to a Docker Engine API socket; the
  shipped `docker-compose.yml` bind-mounts the *host's* socket, which is a
  large privilege-escalation surface. §2a documents safer alternatives
  (rootless Podman, Sysbox) — `DOCKER_SOCKET_PATH` is a real, wired
  override, not just accepted-and-ignored.
