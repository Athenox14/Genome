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
      .query(
        WIKI_PAGES_QUERY,
        { owner: owner.value, repo: repoName.value },
        { requestPolicy: 'network-only' }
      )
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
      <h1 class="text-xl font-semibold text-fg">
        Wiki · {{ owner }}/{{ repoName }}
      </h1>
      <NuxtLink
        :to="`/${owner}/${repoName}/wiki/new`"
        class="gh-btn-primary"
      >
        New page
      </NuxtLink>
    </div>

    <p v-if="error" class="gh-card mb-4 border-danger bg-red-50 p-3 text-sm text-danger">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <ul v-else class="gh-card divide-y divide-border">
      <li v-if="pages.length === 0" class="p-4 text-sm text-fg-muted">No wiki pages yet.</li>
      <li v-for="page in pages" :key="page" class="p-4 hover:bg-canvas-subtle">
        <NuxtLink
          :to="`/${owner}/${repoName}/wiki/${encodeURIComponent(page)}`"
          class="font-medium text-accent hover:underline"
        >
          {{ page }}
        </NuxtLink>
      </li>
    </ul>
  </div>
</template>
