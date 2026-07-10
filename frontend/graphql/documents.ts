/**
 * Plain GraphQL document strings (no codegen), covering the best-guess
 * Genome schema shape. Backend team: please align field names with these
 * where reasonable, or tell us what changed.
 */

export const MY_REPOSITORIES_QUERY = /* GraphQL */ `
  query MyRepositories {
    myRepositories {
      id
      name
      owner
      description
      isPrivate
      defaultBranch
      updatedAt
      starCount
    }
  }
`

export const CREATE_REPOSITORY_MUTATION = /* GraphQL */ `
  mutation CreateRepository($name: String!, $description: String, $isPrivate: Boolean) {
    createRepository(name: $name, description: $description, isPrivate: $isPrivate) {
      id
      name
      owner
    }
  }
`

export const REPO_OVERVIEW_QUERY = /* GraphQL */ `
  query RepoOverview($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      name
      owner
      description
      defaultBranch
      cloneUrlHttp
      cloneUrlSsh
      branches {
        name
      }
    }
  }
`

export const REPO_TREE_QUERY = /* GraphQL */ `
  query RepoTree($owner: String!, $repo: String!, $ref: String!, $path: String) {
    repoTree(owner: $owner, repo: $repo, ref: $ref, path: $path) {
      path
      name
      type
      size
    }
  }
`

export const REPO_README_QUERY = /* GraphQL */ `
  query RepoReadme($owner: String!, $repo: String!, $ref: String!) {
    repoFileContent(owner: $owner, repo: $repo, ref: $ref, path: "README.md") {
      content
      encoding
    }
  }
`

export const ISSUES_QUERY = /* GraphQL */ `
  query Issues($owner: String!, $repo: String!) {
    issues(owner: $owner, repo: $repo) {
      id
      number
      title
      state
      author {
        username
      }
      createdAt
      commentCount
    }
  }
`

export const CREATE_ISSUE_MUTATION = /* GraphQL */ `
  mutation CreateIssue($owner: String!, $repo: String!, $title: String!, $body: String) {
    createIssue(owner: $owner, repo: $repo, title: $title, body: $body) {
      id
      number
      title
    }
  }
`

export const PULL_REQUESTS_QUERY = /* GraphQL */ `
  query PullRequests($owner: String!, $repo: String!) {
    pullRequests(owner: $owner, repo: $repo) {
      id
      number
      title
      state
      sourceBranch
      targetBranch
      author {
        username
      }
      createdAt
    }
  }
`

export const WORKFLOW_RUNS_QUERY = /* GraphQL */ `
  query WorkflowRuns($owner: String!, $repo: String!) {
    workflowRuns(owner: $owner, repo: $repo) {
      id
      runNumber
      workflowName
      status
      conclusion
      branch
      commitSha
      createdAt
    }
  }
`

export const WORKFLOW_RUN_QUERY = /* GraphQL */ `
  query WorkflowRun($owner: String!, $repo: String!, $runId: ID!) {
    workflowRun(owner: $owner, repo: $repo, id: $runId) {
      id
      runNumber
      workflowName
      status
      conclusion
      branch
      commitSha
      jobs {
        id
        name
        status
        conclusion
        logs
      }
    }
  }
`

export const DEV_WORKSPACES_QUERY = /* GraphQL */ `
  query DevWorkspaces {
    devWorkspaces {
      id
      name
      repository
      status
      proxyUrl
      createdAt
    }
  }
`

export const CREATE_DEV_WORKSPACE_MUTATION = /* GraphQL */ `
  mutation CreateDevWorkspace($name: String!, $repository: String, $branch: String) {
    createDevWorkspace(name: $name, repository: $repository, branch: $branch) {
      id
      name
      status
    }
  }
`

export const START_DEV_WORKSPACE_MUTATION = /* GraphQL */ `
  mutation StartDevWorkspace($id: ID!) {
    startDevWorkspace(id: $id) {
      id
      status
      proxyUrl
    }
  }
`

export const STOP_DEV_WORKSPACE_MUTATION = /* GraphQL */ `
  mutation StopDevWorkspace($id: ID!) {
    stopDevWorkspace(id: $id) {
      id
      status
    }
  }
`

export const ORGANIZATIONS_QUERY = /* GraphQL */ `
  query Organizations {
    organizations {
      id
      name
      description
      memberCount
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
