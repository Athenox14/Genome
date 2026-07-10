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
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">Search</h1>

    <form class="mb-6 flex gap-2" @submit.prevent="runSearch">
      <input
        v-model="queryText"
        placeholder="Search repositories, issues, users…"
        class="flex-1 rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
      />
      <button class="rounded bg-gray-900 px-3 py-2 text-sm text-white hover:bg-gray-700">Search</button>
    </form>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Searching…</p>

    <template v-else-if="searched">
      <div class="mb-6">
        <h2 class="mb-2 text-sm font-semibold text-gray-500">Repositories</h2>
        <ul class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
          <li v-if="repositories.length === 0" class="p-3 text-sm text-gray-500">No matches.</li>
          <li v-for="repo in repositories" :key="repo.id" class="p-3">
            <NuxtLink :to="`/${repo.ownerLogin}/${repo.name}`" class="font-medium text-blue-600 hover:underline dark:text-blue-400">
              {{ repo.ownerLogin }}/{{ repo.name }}
            </NuxtLink>
            <p v-if="repo.description" class="text-xs text-gray-500">{{ repo.description }}</p>
          </li>
        </ul>
      </div>

      <div class="mb-6">
        <h2 class="mb-2 text-sm font-semibold text-gray-500">Issues</h2>
        <ul class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
          <li v-if="issues.length === 0" class="p-3 text-sm text-gray-500">No matches.</li>
          <li v-for="issue in issues" :key="issue.id" class="p-3 text-sm text-gray-900 dark:text-white">
            #{{ issue.number }} {{ issue.title }}
          </li>
        </ul>
      </div>

      <div>
        <h2 class="mb-2 text-sm font-semibold text-gray-500">Users</h2>
        <ul class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
          <li v-if="users.length === 0" class="p-3 text-sm text-gray-500">No matches.</li>
          <li v-for="user in users" :key="user.id" class="p-3 text-sm text-gray-900 dark:text-white">
            {{ user.username }}
          </li>
        </ul>
      </div>
    </template>
  </div>
</template>
