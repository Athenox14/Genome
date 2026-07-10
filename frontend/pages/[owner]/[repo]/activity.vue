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
    <h1 class="mb-4 text-xl font-semibold text-fg">
      Activity · {{ owner }}/{{ repoName }}
    </h1>

    <p v-if="error" class="gh-card mb-4 border-danger bg-red-50 p-3 text-sm text-danger">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <ul v-else class="gh-card divide-y divide-border">
      <li v-if="events.length === 0" class="p-4 text-sm text-fg-muted">No activity yet.</li>
      <li v-for="event in events" :key="event.id" class="p-4">
        <div class="flex items-center justify-between">
          <span class="text-sm text-fg">{{ event.summary }}</span>
          <span class="text-xs text-fg-muted">{{ new Date(event.createdAt).toLocaleString() }}</span>
        </div>
        <span class="text-xs uppercase tracking-wide text-fg-muted">{{ event.kind }}</span>
      </li>
    </ul>
  </div>
</template>
