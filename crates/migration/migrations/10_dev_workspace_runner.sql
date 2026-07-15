-- Lets a dev workspace be hosted by a standalone `runner` process instead
-- of being created directly against the `server` process's own Docker
-- daemon (see `entity::runner_job::kind::DEV_WORKSPACE_ACTION` and
-- `runner/src/dev_workspace_poll.rs`). `runner_id` is a free-form
-- identifier the hosting runner process self-reports (there is no runner
-- registry/heartbeat table) -- it is NULL for server-hosted workspaces
-- (the existing, default behavior) and set once a runner claims and
-- creates the workspace.
ALTER TABLE dev_workspaces ADD COLUMN runner_id TEXT;

-- Carries a runner's completion result (e.g. the created container's id,
-- or an `execInDevWorkspace` command's output) back through
-- `POST /runner/jobs/:id/complete` -- previously completion only recorded
-- `status`/`finished_at`, with the request body's `logs` field discarded,
-- which was fine when nothing needed the result of a CI job fed back into
-- another read, but a dev-workspace action's caller (a GraphQL mutation
-- polling this row) does need it.
ALTER TABLE runner_jobs ADD COLUMN result TEXT;
