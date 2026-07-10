<script setup lang="ts">
import { WIKI_PAGES_QUERY } from '~/graphql/documents'

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const pages = ref<string[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

async function loadPages() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql
      .query(WIKI_PAGES_QUERY, { owner: owner.value, repo: repoName.value })
      .toPromise()
    if (result.error) throw result.error
    pages.value = result.data?.repository?.wikiPages ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load wiki pages'
  } finally {
    loading.value = false
  }
}

onMounted(loadPages)
</script>

<template>
  <div>
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">
        Wiki · {{ owner }}/{{ repoName }}
      </h1>
      <NuxtLink
        :to="`/${owner}/${repoName}/wiki/new`"
        class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
      >
        New page
      </NuxtLink>
    </div>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <ul v-else class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
      <li v-if="pages.length === 0" class="p-4 text-sm text-gray-500">No wiki pages yet.</li>
      <li v-for="page in pages" :key="page" class="p-4">
        <NuxtLink
          :to="`/${owner}/${repoName}/wiki/${encodeURIComponent(page)}`"
          class="font-medium text-blue-600 hover:underline dark:text-blue-400"
        >
          {{ page }}
        </NuxtLink>
      </li>
    </ul>
  </div>
</template>
