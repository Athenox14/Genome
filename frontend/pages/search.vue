<script setup lang="ts">
import { SEARCH_QUERY } from '~/graphql/documents'

const route = useRoute()
const router = useRouter()
const { $urql } = useNuxtApp()

const queryText = ref(String(route.query.q || ''))
const repositories = ref<{ id: string; name: string; ownerLogin: string; description: string | null }[]>([])
const issues = ref<{ id: string; number: number; title: string; repoId: string }[]>([])
const users = ref<{ id: string; username: string }[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const searched = ref(false)

async function runSearch() {
  if (!queryText.value.trim()) return
  loading.value = true
  error.value = null
  searched.value = true
  router.replace({ query: { q: queryText.value } })
  try {
    const result = await $urql.query(SEARCH_QUERY, { query: queryText.value }).toPromise()
    if (result.error) throw result.error
    repositories.value = result.data?.search?.repositories ?? []
    issues.value = result.data?.search?.issues ?? []
    users.value = result.data?.search?.users ?? []
  } catch (err: any) {
    error.value = err?.message || 'Search failed'
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  if (queryText.value) runSearch()
})
</script>

<template>
  <div>
    <h1 class="mb-4 text-xl font-semibold text-fg">Search</h1>

    <form class="mb-6 flex gap-2" @submit.prevent="runSearch">
      <input
        v-model="queryText"
        placeholder="Search repositories, issues, users…"
        class="flex-1 rounded border border-border px-3 py-2 text-sm text-fg"
      />
      <button class="gh-btn-primary">Search</button>
    </form>

    <p v-if="error" class="mb-4 text-sm text-danger-emphasis">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Searching…</p>

    <template v-else-if="searched">
      <div class="mb-6">
        <h2 class="mb-2 text-sm font-semibold text-fg-muted">Repositories</h2>
        <ul class="gh-card divide-y divide-border">
          <li v-if="repositories.length === 0" class="p-3 text-sm text-fg-muted">No matches.</li>
          <li v-for="repo in repositories" :key="repo.id" class="p-3">
            <NuxtLink :to="`/${repo.ownerLogin}/${repo.name}`" class="font-medium text-accent hover:underline">
              {{ repo.ownerLogin }}/{{ repo.name }}
            </NuxtLink>
            <p v-if="repo.description" class="text-xs text-fg-muted">{{ repo.description }}</p>
          </li>
        </ul>
      </div>

      <div class="mb-6">
        <h2 class="mb-2 text-sm font-semibold text-fg-muted">Issues</h2>
        <ul class="gh-card divide-y divide-border">
          <li v-if="issues.length === 0" class="p-3 text-sm text-fg-muted">No matches.</li>
          <li v-for="issue in issues" :key="issue.id" class="p-3 text-sm text-fg">
            #{{ issue.number }} {{ issue.title }}
          </li>
        </ul>
      </div>

      <div>
        <h2 class="mb-2 text-sm font-semibold text-fg-muted">Users</h2>
        <ul class="gh-card divide-y divide-border">
          <li v-if="users.length === 0" class="p-3 text-sm text-fg-muted">No matches.</li>
          <li v-for="user in users" :key="user.id" class="p-3 text-sm text-fg">
            {{ user.username }}
          </li>
        </ul>
      </div>
    </template>
  </div>
</template>
