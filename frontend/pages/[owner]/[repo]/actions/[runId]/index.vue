<script setup lang="ts">
import { WORKFLOW_RUN_QUERY } from '~/graphql/documents'

interface Job {
  id: string
  name: string
  status: string
  conclusion: string | null
  logs: string | null
}

interface WorkflowRunDetail {
  id: string
  runNumber: number
  workflowName: string
  status: string
  conclusion: string | null
  branch: string
  commitSha: string
  jobs: Job[]
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))
const runId = computed(() => String(route.params.runId))

const { $urql } = useNuxtApp()

const run = ref<WorkflowRunDetail | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)
const selectedJobId = ref<string | null>(null)
let pollTimer: ReturnType<typeof setInterval> | null = null

const selectedJob = computed(() => run.value?.jobs.find((j) => j.id === selectedJobId.value) ?? null)

async function loadRun() {
  error.value = null
  try {
    const result = await $urql
      .query(WORKFLOW_RUN_QUERY, { owner: owner.value, repo: repoName.value, runId: runId.value })
      .toPromise()
    if (result.error) throw result.error
    run.value = result.data?.workflowRun ?? null
    if (run.value && !selectedJobId.value && run.value.jobs.length > 0) {
      selectedJobId.value = run.value.jobs[0].id
    }
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
        {{ run.workflowName }} #{{ run.runNumber }}
        <span class="ml-2 text-sm font-normal text-gray-500">
          ({{ run.conclusion || run.status }})
        </span>
      </h1>

      <div class="grid grid-cols-3 gap-4">
        <div class="col-span-1 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
          <div class="border-b border-gray-200 p-2 text-sm font-medium text-gray-700 dark:border-gray-800 dark:text-gray-300">
            Jobs
          </div>
          <ul class="divide-y divide-gray-100 dark:divide-gray-800">
            <li
              v-for="job in run.jobs"
              :key="job.id"
              class="cursor-pointer p-2 text-sm hover:bg-gray-50 dark:hover:bg-gray-800"
              :class="{ 'bg-gray-100 dark:bg-gray-800': job.id === selectedJobId }"
              @click="selectedJobId = job.id"
            >
              {{ job.name }}
              <span class="ml-1 text-xs text-gray-500">({{ job.conclusion || job.status }})</span>
            </li>
          </ul>
        </div>

        <div class="col-span-2 rounded border border-gray-200 bg-gray-950 p-4 dark:border-gray-800">
          <pre class="whitespace-pre-wrap text-xs text-green-400">{{ selectedJob?.logs || 'No logs available.' }}</pre>
        </div>
      </div>
    </template>
  </div>
</template>
