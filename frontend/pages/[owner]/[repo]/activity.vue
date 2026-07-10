<script setup lang="ts">
import { REPO_ACTIVITY_QUERY } from '~/graphql/documents'

interface ActivityEvent {
  id: string
  kind: string
  summary: string
  createdAt: string
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const events = ref<ActivityEvent[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

async function load() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql
      .query(REPO_ACTIVITY_QUERY, { owner: owner.value, repo: repoName.value, limit: 50 })
      .toPromise()
    if (result.error) throw result.error
    events.value = result.data?.repository?.activity ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load activity'
  } finally {
    loading.value = false
  }
}

onMounted(load)
</script>

<template>
  <div>
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">
      Activity · {{ owner }}/{{ repoName }}
    </h1>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <ul v-else class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
      <li v-if="events.length === 0" class="p-4 text-sm text-gray-500">No activity yet.</li>
      <li v-for="event in events" :key="event.id" class="p-4">
        <div class="flex items-center justify-between">
          <span class="text-sm text-gray-900 dark:text-white">{{ event.summary }}</span>
          <span class="text-xs text-gray-400">{{ new Date(event.createdAt).toLocaleString() }}</span>
        </div>
        <span class="text-xs uppercase tracking-wide text-gray-400">{{ event.kind }}</span>
      </li>
    </ul>
  </div>
</template>
