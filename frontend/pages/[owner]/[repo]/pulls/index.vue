<script setup lang="ts">
import { PULL_REQUESTS_QUERY } from '~/graphql/documents'

interface PullRequest {
  id: string
  number: number
  title: string
  state: string
  sourceBranch: string
  targetBranch: string
  authorId: string
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
      .query(PULL_REQUESTS_QUERY, { owner: owner.value, repo: repoName.value }, { requestPolicy: 'network-only' })
      .toPromise()
    if (result.error) throw result.error
    pulls.value = result.data?.repository?.pullRequests ?? []
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
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-fg">
        Pull requests · {{ owner }}/{{ repoName }}
      </h1>
    </div>

    <p v-if="error" class="mb-4 text-sm text-danger-emphasis">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <ul v-else class="gh-card divide-y divide-border">
      <li v-if="pulls.length === 0" class="p-4 text-sm text-fg-muted">No pull requests yet.</li>
      <li v-for="pr in pulls" :key="pr.id" class="p-4">
        <div class="flex items-center gap-2">
          <svg
            class="h-3 w-3 shrink-0"
            :class="pr.state === 'open' ? 'text-success' : pr.state === 'merged' ? 'text-done' : 'text-danger'"
            viewBox="0 0 16 16"
            fill="currentColor"
          >
            <circle cx="8" cy="8" r="8" />
          </svg>
          <span class="font-medium text-accent hover:underline">#{{ pr.number }} {{ pr.title }}</span>
        </div>
        <p class="mt-1 pl-5 text-xs text-fg-muted">
          {{ pr.sourceBranch }} → {{ pr.targetBranch }} · opened {{ new Date(pr.createdAt).toLocaleDateString() }}
        </p>
      </li>
    </ul>
  </div>
</template>
