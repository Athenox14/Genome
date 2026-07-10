<script setup lang="ts">
import { PULL_REQUESTS_QUERY } from '~/graphql/documents'

interface PullRequest {
  id: string
  number: number
  title: string
  state: string
  sourceBranch: string
  targetBranch: string
  author: { username: string } | null
  createdAt: string
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const pulls = ref<PullRequest[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

async function loadPulls() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql
      .query(PULL_REQUESTS_QUERY, { owner: owner.value, repo: repoName.value })
      .toPromise()
    if (result.error) throw result.error
    pulls.value = result.data?.pullRequests ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load pull requests'
  } finally {
    loading.value = false
  }
}

onMounted(loadPulls)
</script>

<template>
  <div>
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">
      Pull requests · {{ owner }}/{{ repoName }}
    </h1>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <ul v-else class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
      <li v-if="pulls.length === 0" class="p-4 text-sm text-gray-500">No pull requests yet.</li>
      <li v-for="pr in pulls" :key="pr.id" class="p-4">
        <span
          class="mr-2 rounded px-1.5 py-0.5 text-xs font-medium"
          :class="pr.state === 'open' ? 'bg-green-100 text-green-700' : 'bg-purple-100 text-purple-700'"
        >
          {{ pr.state }}
        </span>
        <span class="font-medium text-gray-900 dark:text-white">#{{ pr.number }} {{ pr.title }}</span>
        <p class="text-xs text-gray-500">
          {{ pr.sourceBranch }} → {{ pr.targetBranch }} by {{ pr.author?.username || 'unknown' }}
        </p>
      </li>
    </ul>
  </div>
</template>
