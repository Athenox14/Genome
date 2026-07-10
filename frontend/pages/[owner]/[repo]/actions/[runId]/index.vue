<script setup lang="ts">
import { WORKFLOW_RUNS_QUERY } from '~/graphql/documents'

interface WorkflowRunDetail {
  id: string
  workflowName: string
  status: string
  event: string
  commitSha: string
  startedAt: string | null
  finishedAt: string | null
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))
const runId = computed(() => String(route.params.runId))

const { $urql } = useNuxtApp()

const run = ref<WorkflowRunDetail | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)
let pollTimer: ReturnType<typeof setInterval> | null = null

// NOTE: the backend has no single `workflowRun(id)` query and
// WorkflowRunObject exposes no `jobs`/logs, so this page loads the run list
// for the repository and finds the matching run by id.
async function loadRun() {
  error.value = null
  try {
    const result = await $urql
      .query(WORKFLOW_RUNS_QUERY, { owner: owner.value, repo: repoName.value })
      .toPromise()
    if (result.error) throw result.error
    const runs: WorkflowRunDetail[] = result.data?.repository?.workflowRuns ?? []
    run.value = runs.find((r) => r.id === runId.value) ?? null
    if (!run.value) error.value = 'Workflow run not found'
  } catch (err: any) {
    error.value = err?.message || 'Failed to load workflow run'
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  loadRun()
  pollTimer = setInterval(loadRun, 5000)
})

onBeforeUnmount(() => {
  if (pollTimer) clearInterval(pollTimer)
})
</script>

<template>
  <div>
    <NuxtLink :to="`/${owner}/${repoName}/actions`" class="mb-4 inline-block text-sm text-blue-600 hover:underline">
      ← Back to runs
    </NuxtLink>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <template v-else-if="run">
      <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">
        {{ run.workflowName }}
        <span class="ml-2 text-sm font-normal text-gray-500">({{ run.status }})</span>
      </h1>

      <div class="rounded border border-gray-200 bg-white p-4 text-sm dark:bg-gray-900 dark:border-gray-800">
        <p><span class="font-medium text-gray-700 dark:text-gray-300">Event:</span> {{ run.event }}</p>
        <p><span class="font-medium text-gray-700 dark:text-gray-300">Commit:</span> {{ run.commitSha }}</p>
        <p v-if="run.startedAt"><span class="font-medium text-gray-700 dark:text-gray-300">Started:</span> {{ run.startedAt }}</p>
        <p v-if="run.finishedAt"><span class="font-medium text-gray-700 dark:text-gray-300">Finished:</span> {{ run.finishedAt }}</p>
      </div>
    </template>
  </div>
</template>
