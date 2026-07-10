<script setup lang="ts">
import { REPO_OVERVIEW_QUERY, REPO_TREE_QUERY, REPO_README_QUERY } from '~/graphql/documents'

interface TreeEntry {
  path: string
  name: string
  type: 'file' | 'dir' | string
  size: number | null
}

interface RepoOverview {
  id: string
  name: string
  owner: string
  description: string | null
  defaultBranch: string
  cloneUrlHttp: string
  cloneUrlSsh: string | null
  branches: { name: string }[]
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const overview = ref<RepoOverview | null>(null)
const tree = ref<TreeEntry[]>([])
const readme = ref<string | null>(null)
const currentPath = ref('')
const currentRef = ref('')
const loading = ref(true)
const error = ref<string | null>(null)

async function loadOverview() {
  const result = await $urql
    .query(REPO_OVERVIEW_QUERY, { owner: owner.value, repo: repoName.value })
    .toPromise()
  if (result.error) throw result.error
  overview.value = result.data?.repository ?? null
  currentRef.value = overview.value?.defaultBranch || 'main'
}

async function loadTree() {
  const result = await $urql
    .query(REPO_TREE_QUERY, {
      owner: owner.value,
      repo: repoName.value,
      ref: currentRef.value,
      path: currentPath.value || null
    })
    .toPromise()
  if (result.error) throw result.error
  tree.value = result.data?.repoTree ?? []
}

async function loadReadme() {
  const result = await $urql
    .query(REPO_README_QUERY, { owner: owner.value, repo: repoName.value, ref: currentRef.value })
    .toPromise()
  readme.value = result.data?.repoFileContent?.content ?? null
}

async function loadAll() {
  loading.value = true
  error.value = null
  try {
    await loadOverview()
    await Promise.all([loadTree(), loadReadme()])
  } catch (err: any) {
    error.value = err?.message || 'Failed to load repository'
  } finally {
    loading.value = false
  }
}

function openEntry(entry: TreeEntry) {
  if (entry.type === 'dir') {
    currentPath.value = entry.path
    loadTree()
  }
}

function goUp() {
  const parts = currentPath.value.split('/').filter(Boolean)
  parts.pop()
  currentPath.value = parts.join('/')
  loadTree()
}

function onBranchChange() {
  loadTree()
  loadReadme()
}

onMounted(loadAll)
</script>

<template>
  <div>
    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <template v-else-if="overview">
      <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 class="text-xl font-semibold text-gray-900 dark:text-white">
            {{ overview.owner }}/{{ overview.name }}
          </h1>
          <p v-if="overview.description" class="text-sm text-gray-600 dark:text-gray-400">
            {{ overview.description }}
          </p>
        </div>

        <select
          v-model="currentRef"
          class="rounded border border-gray-300 px-2 py-1 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
          @change="onBranchChange"
        >
          <option v-for="b in overview.branches" :key="b.name" :value="b.name">{{ b.name }}</option>
        </select>
      </div>

      <div class="mb-6 flex gap-6 border-b border-gray-200 text-sm dark:border-gray-800">
        <NuxtLink :to="`/${owner}/${repoName}`" class="border-b-2 border-gray-900 pb-2 font-medium dark:border-white dark:text-white">
          Code
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/issues`" class="pb-2 text-gray-600 hover:text-gray-900 dark:text-gray-400">
          Issues
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/pulls`" class="pb-2 text-gray-600 hover:text-gray-900 dark:text-gray-400">
          Pull requests
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/actions`" class="pb-2 text-gray-600 hover:text-gray-900 dark:text-gray-400">
          Actions
        </NuxtLink>
      </div>

      <div class="mb-4 rounded border border-gray-200 bg-white p-3 text-sm dark:bg-gray-900 dark:border-gray-800">
        <span class="font-medium text-gray-700 dark:text-gray-300">Clone: </span>
        <code class="rounded bg-gray-100 px-2 py-1 dark:bg-gray-800">{{ overview.cloneUrlHttp }}</code>
      </div>

      <div class="rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
        <div class="flex items-center gap-2 border-b border-gray-200 p-2 text-sm dark:border-gray-800">
          <button v-if="currentPath" class="text-blue-600 hover:underline" @click="goUp">.. (up)</button>
          <span class="text-gray-500">/{{ currentPath }}</span>
        </div>
        <ul class="divide-y divide-gray-100 dark:divide-gray-800">
          <li
            v-for="entry in tree"
            :key="entry.path"
            class="cursor-pointer p-2 text-sm hover:bg-gray-50 dark:hover:bg-gray-800"
            @click="openEntry(entry)"
          >
            <span>{{ entry.type === 'dir' ? '📁' : '📄' }}</span>
            {{ entry.name }}
          </li>
        </ul>
      </div>

      <div v-if="readme" class="mt-6 rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800">
        <h2 class="mb-2 text-sm font-semibold text-gray-700 dark:text-gray-300">README.md</h2>
        <pre class="whitespace-pre-wrap text-sm text-gray-800 dark:text-gray-200">{{ readme }}</pre>
      </div>
    </template>
  </div>
</template>
