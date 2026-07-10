/**
 * Plain GraphQL document strings (no codegen), aligned to the real backend
 * schema in crates/graphql-api/src/{query,mutation,types}.rs.
 *
 * Notes on schema shape:
 * - RepositoryObject exposes `ownerLogin` (username or org name), resolved
 *   dynamically from the polymorphic `ownerType`/`ownerId` pair. Use it to
 *   build `/{ownerLogin}/{name}` links instead of falling back to raw
 *   `ownerId` UUIDs.
 * - Issues/PullRequests/WorkflowRuns/Branches/Tree/Commits are only
 *   reachable as nested fields on `repository(owner, name)`, not as root
 *   query fields.
 * - IssueObject/PullRequestObject only expose `authorId` (no nested author
 *   object) and have no comment/job sub-resolvers.
 * - There is no single `workflowRun(id)` query, and WorkflowRunObject has no
 *   `jobs`, `runNumber`, `conclusion`, or `branch` fields.
 * - DevWorkspaceObject has no `proxyUrl` or `repository` (string) field;
 *   it has `repoId` (UUID) and `image`.
 * - Mutations that touch a repo (createIssue, createPullRequest,
 *   triggerWorkflowDispatch, addCollaborator, deleteRepository) take a
 *   `repoId: UUID!`, not owner/repo strings.
 */

export const MY_REPOSITORIES_QUERY = /* GraphQL */ `
  query MyRepositories {
    myRepositories {
      id
      ownerType
      ownerId
      ownerLogin
      name
      description
      isPrivate
      defaultBranch
      createdAt
    }
  }
`

export const CREATE_REPOSITORY_MUTATION = /* GraphQL */ `
  mutation CreateRepository($name: String!, $description: String, $isPrivate: Boolean!) {
    createRepository(name: $name, description: $description, isPrivate: $isPrivate) {
      id
      ownerType
      ownerId
      ownerLogin
      name
    }
  }
`

export const REPO_OVERVIEW_QUERY = /* GraphQL */ `
  query RepoOverview($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      ownerType
      ownerId
      ownerLogin
      name
      description
      defaultBranch
      branches {
        name
      }
    }
  }
`

export const REPO_TREE_QUERY = /* GraphQL */ `
  query RepoTree($owner: String!, $repo: String!, $ref: String!, $path: String!) {
    repository(owner: $owner, name: $repo) {
      tree(ref: $ref, path: $path) {
        name
        path
        kind
        size
        oid
      }
    }
  }
`

export const ISSUES_QUERY = /* GraphQL */ `
  query Issues($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      issues {
        id
        number
        title
        state
        authorId
        createdAt
      }
    }
  }
`

export const CREATE_ISSUE_MUTATION = /* GraphQL */ `
  mutation CreateIssue($repoId: UUID!, $title: String!, $body: String) {
    createIssue(repoId: $repoId, title: $title, body: $body) {
      id
      number
      title
    }
  }
`

export const PULL_REQUESTS_QUERY = /* GraphQL */ `
  query PullRequests($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      pullRequests {
        id
        number
        title
        state
        sourceBranch
        targetBranch
        authorId
        createdAt
      }
    }
  }
`

export const WORKFLOW_RUNS_QUERY = /* GraphQL */ `
  query WorkflowRuns($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      workflowRuns {
        id
        workflowName
        status
        event
        commitSha
        startedAt
        finishedAt
      }
    }
  }
`

export const TRIGGER_WORKFLOW_DISPATCH_MUTATION = /* GraphQL */ `
  mutation TriggerWorkflowDispatch($repoId: UUID!, $workflowPath: String!) {
    triggerWorkflowDispatch(repoId: $repoId, workflowPath: $workflowPath) {
      id
      workflowName
      status
    }
  }
`

export const DEV_WORKSPACES_QUERY = /* GraphQL */ `
  query DevWorkspaces {
    devWorkspaces {
      id
      repoId
      name
      image
      status
      createdAt
    }
  }
`

export const CREATE_DEV_WORKSPACE_MUTATION = /* GraphQL */ `
  mutation CreateDevWorkspace($name: String!, $image: String, $repoId: UUID) {
    createDevWorkspace(name: $name, image: $image, repoId: $repoId) {
      id
      name
      status
    }
  }
`

export const START_DEV_WORKSPACE_MUTATION = /* GraphQL */ `
  mutation StartDevWorkspace($workspaceId: UUID!) {
    startDevWorkspace(workspaceId: $workspaceId) {
      id
      status
    }
  }
`

export const STOP_DEV_WORKSPACE_MUTATION = /* GraphQL */ `
  mutation StopDevWorkspace($workspaceId: UUID!) {
    stopDevWorkspace(workspaceId: $workspaceId) {
      id
      status
    }
  }
`

export const DELETE_DEV_WORKSPACE_MUTATION = /* GraphQL */ `
  mutation DeleteDevWorkspace($workspaceId: UUID!) {
    deleteDevWorkspace(workspaceId: $workspaceId)
  }
`

export const ORGANIZATIONS_QUERY = /* GraphQL */ `
  query Organizations {
    organizations {
      id
      name
      description
      createdAt
    }
  }
`

export const CREATE_ORGANIZATION_MUTATION = /* GraphQL */ `
  mutation CreateOrganization($name: String!, $description: String) {
    createOrganization(name: $name, description: $description) {
      id
      name
    }
  }
`
