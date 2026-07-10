<script setup lang="ts">
import { WORKFLOW_RUNS_QUERY } from '~/graphql/documents'

interface WorkflowRun {
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

const { $urql } = useNuxtApp()

const runs = ref<WorkflowRun[]>([])
const loading = ref(true)
const error = ref<string | null>(null)
let pollTimer: ReturnType<typeof setInterval> | null = null

function statusBadgeClass(status: string) {
  if (status === 'queued' || status === 'in_progress') return 'bg-yellow-100 text-yellow-700'
  if (status === 'success') return 'bg-green-100 text-green-700'
  if (status === 'failure' || status === 'error') return 'bg-red-100 text-red-700'
  return 'bg-gray-100 text-gray-700'
}

async function loadRuns() {
  error.value = null
  try {
    const result = await $urql
      .query(WORKFLOW_RUNS_QUERY, { owner: owner.value, repo: repoName.value })
      .toPromise()
    if (result.error) throw result.error
    runs.value = result.data?.repository?.workflowRuns ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load workflow runs'
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  loadRuns()
  pollTimer = setInterval(loadRuns, 5000)
})

onBeforeUnmount(() => {
  if (pollTimer) clearInterval(pollTimer)
})
</script>

<template>
  <div>
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">
      Actions · {{ owner }}/{{ repoName }}
    </h1>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <ul v-else class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
      <li v-if="runs.length === 0" class="p-4 text-sm text-gray-500">No workflow runs yet.</li>
      <li
        v-for="run in runs"
        :key="run.id"
        class="cursor-pointer p-4 hover:bg-gray-50 dark:hover:bg-gray-800"
      >
        <NuxtLink :to="`/${owner}/${repoName}/actions/${run.id}`" class="flex items-center justify-between">
          <div>
            <span
              class="mr-2 rounded px-1.5 py-0.5 text-xs font-medium"
              :class="statusBadgeClass(run.status)"
            >
              {{ run.status }}
            </span>
            <span class="font-medium text-gray-900 dark:text-white">
              {{ run.workflowName }}
            </span>
            <p class="text-xs text-gray-500">
              {{ run.event }} @ {{ run.commitSha.slice(0, 7) }}
            </p>
          </div>
        </NuxtLink>
      </li>
    </ul>
  </div>
</template>
