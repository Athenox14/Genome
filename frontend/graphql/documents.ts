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

// ---- Wiki ----

export const WIKI_PAGES_QUERY = /* GraphQL */ `
  query WikiPages($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      wikiPages
    }
  }
`

export const WIKI_PAGE_QUERY = /* GraphQL */ `
  query WikiPage($owner: String!, $repo: String!, $page: String!) {
    repository(owner: $owner, name: $repo) {
      id
      wikiPage(page: $page)
    }
  }
`

export const WRITE_WIKI_PAGE_MUTATION = /* GraphQL */ `
  mutation WriteWikiPage($repoId: UUID!, $page: String!, $content: String!, $message: String!) {
    writeWikiPage(repoId: $repoId, page: $page, content: $content, message: $message)
  }
`

// ---- Labels ----

export const REPO_LABELS_QUERY = /* GraphQL */ `
  query RepoLabels($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      labels {
        id
        name
        color
      }
    }
  }
`

export const CREATE_LABEL_MUTATION = /* GraphQL */ `
  mutation CreateLabel($repoId: UUID!, $name: String!, $color: String!) {
    createLabel(repoId: $repoId, name: $name, color: $color) {
      id
      name
      color
    }
  }
`

export const ADD_LABEL_TO_ISSUE_MUTATION = /* GraphQL */ `
  mutation AddLabelToIssue($issueId: UUID!, $labelId: UUID!) {
    addLabelToIssue(issueId: $issueId, labelId: $labelId) {
      id
    }
  }
`

export const REMOVE_LABEL_FROM_ISSUE_MUTATION = /* GraphQL */ `
  mutation RemoveLabelFromIssue($issueId: UUID!, $labelId: UUID!) {
    removeLabelFromIssue(issueId: $issueId, labelId: $labelId) {
      id
    }
  }
`

// ---- Milestones ----

export const REPO_MILESTONES_QUERY = /* GraphQL */ `
  query RepoMilestones($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      milestones {
        id
        title
        state
        dueDate
      }
    }
  }
`

export const CREATE_MILESTONE_MUTATION = /* GraphQL */ `
  mutation CreateMilestone($repoId: UUID!, $title: String!, $description: String, $dueDate: DateTime) {
    createMilestone(repoId: $repoId, title: $title, description: $description, dueDate: $dueDate) {
      id
      title
    }
  }
`

export const SET_ISSUE_MILESTONE_MUTATION = /* GraphQL */ `
  mutation SetIssueMilestone($issueId: UUID!, $milestoneId: UUID) {
    setIssueMilestone(issueId: $issueId, milestoneId: $milestoneId) {
      id
    }
  }
`

// ---- Issues (with labels + milestone) ----

export const ISSUES_WITH_META_QUERY = /* GraphQL */ `
  query IssuesWithMeta($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      labels {
        id
        name
        color
      }
      milestones {
        id
        title
      }
      issues {
        id
        number
        title
        state
        authorId
        createdAt
        labels {
          id
          name
          color
        }
        milestone {
          id
          title
        }
      }
    }
  }
`

// ---- Projects / kanban ----

export const REPO_PROJECTS_QUERY = /* GraphQL */ `
  query RepoProjects($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      projects {
        id
        name
        columns {
          id
          name
          position
          cards {
            id
            position
            issueId
            issue {
              id
              number
              title
            }
          }
        }
      }
    }
  }
`

export const CREATE_PROJECT_MUTATION = /* GraphQL */ `
  mutation CreateProject($repoId: UUID!, $name: String!) {
    createProject(repoId: $repoId, name: $name) {
      id
      name
    }
  }
`

export const ADD_PROJECT_COLUMN_MUTATION = /* GraphQL */ `
  mutation AddProjectColumn($projectId: UUID!, $name: String!, $position: Int!) {
    addProjectColumn(projectId: $projectId, name: $name, position: $position) {
      id
      name
      position
    }
  }
`

export const ADD_CARD_TO_COLUMN_MUTATION = /* GraphQL */ `
  mutation AddCardToColumn($columnId: UUID!, $issueId: UUID!, $position: Int!) {
    addCardToColumn(columnId: $columnId, issueId: $issueId, position: $position) {
      id
      position
    }
  }
`

// ---- Two-factor authentication ----

export const ENABLE_TWO_FACTOR_MUTATION = /* GraphQL */ `
  mutation EnableTwoFactor {
    enableTwoFactor {
      secret
      provisioningUri
    }
  }
`

export const CONFIRM_TWO_FACTOR_MUTATION = /* GraphQL */ `
  mutation ConfirmTwoFactor($code: String!) {
    confirmTwoFactor(code: $code)
  }
`

export const DISABLE_TWO_FACTOR_MUTATION = /* GraphQL */ `
  mutation DisableTwoFactor {
    disableTwoFactor
  }
`

// ---- Admin ----

export const ADMIN_LIST_USERS_QUERY = /* GraphQL */ `
  query AdminListUsers($limit: Int!, $offset: Int!) {
    adminListUsers(limit: $limit, offset: $offset) {
      id
      username
      email
      isAdmin
      createdAt
      deactivatedAt
    }
  }
`

export const ADMIN_SET_USER_ADMIN_MUTATION = /* GraphQL */ `
  mutation AdminSetUserAdmin($userId: UUID!, $isAdmin: Boolean!) {
    adminSetUserAdmin(userId: $userId, isAdmin: $isAdmin) {
      id
      isAdmin
    }
  }
`

export const ADMIN_DEACTIVATE_USER_MUTATION = /* GraphQL */ `
  mutation AdminDeactivateUser($userId: UUID!) {
    adminDeactivateUser(userId: $userId) {
      id
    }
  }
`

// ---- Branch protection ----

export const REPO_BRANCH_PROTECTION_RULES_QUERY = /* GraphQL */ `
  query RepoBranchProtectionRules($owner: String!, $repo: String!) {
    repository(owner: $owner, name: $repo) {
      id
      branchProtectionRules {
        id
        branchPattern
        requireReviewsCount
        blockForcePush
      }
    }
  }
`

export const CREATE_BRANCH_PROTECTION_RULE_MUTATION = /* GraphQL */ `
  mutation CreateBranchProtectionRule(
    $repoId: UUID!
    $branchPattern: String!
    $requireReviewsCount: Int!
    $blockForcePush: Boolean!
  ) {
    createBranchProtectionRule(
      repoId: $repoId
      branchPattern: $branchPattern
      requireReviewsCount: $requireReviewsCount
      blockForcePush: $blockForcePush
    ) {
      id
      branchPattern
      requireReviewsCount
      blockForcePush
    }
  }
`

// ---- PR reviews ----

export const SUBMIT_PULL_REQUEST_REVIEW_MUTATION = /* GraphQL */ `
  mutation SubmitPullRequestReview($prId: UUID!, $state: String!, $body: String) {
    submitPullRequestReview(prId: $prId, state: $state, body: $body) {
      id
      state
    }
  }
`

export const ADD_REVIEW_COMMENT_MUTATION = /* GraphQL */ `
  mutation AddReviewComment($reviewId: UUID!, $filePath: String!, $lineNumber: Int!, $body: String!) {
    addReviewComment(reviewId: $reviewId, filePath: $filePath, lineNumber: $lineNumber, body: $body) {
      id
      body
    }
  }
`

// ---- Notifications & activity ----

export const MY_NOTIFICATIONS_QUERY = /* GraphQL */ `
  query MyNotifications($unreadOnly: Boolean) {
    myNotifications(unreadOnly: $unreadOnly) {
      id
      kind
      message
      readAt
      createdAt
      repoId
      subjectId
    }
  }
`

export const MARK_NOTIFICATION_READ_MUTATION = /* GraphQL */ `
  mutation MarkNotificationRead($id: UUID!) {
    markNotificationRead(id: $id) {
      id
      readAt
    }
  }
`

export const MY_ACTIVITY_QUERY = /* GraphQL */ `
  query MyActivity($limit: Int!) {
    myActivity(limit: $limit) {
      id
      kind
      summary
      createdAt
      repoId
    }
  }
`

export const REPO_ACTIVITY_QUERY = /* GraphQL */ `
  query RepoActivity($owner: String!, $repo: String!, $limit: Int!) {
    repository(owner: $owner, name: $repo) {
      id
      activity(limit: $limit) {
        id
        kind
        summary
        createdAt
      }
    }
  }
`

// ---- Search ----

export const SEARCH_QUERY = /* GraphQL */ `
  query Search($query: String!) {
    search(query: $query) {
      repositories {
        id
        name
        ownerLogin
        description
      }
      issues {
        id
        number
        title
        repoId
      }
      users {
        id
        username
      }
    }
  }
`
