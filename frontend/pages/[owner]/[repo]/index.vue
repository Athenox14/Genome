<script setup lang="ts">
import { REPO_OVERVIEW_QUERY, REPO_TREE_QUERY } from '~/graphql/documents'

interface TreeEntry {
  path: string
  name: string
  kind: 'file' | 'dir' | string
  size: number | null
  oid: string
}

interface RepoOverview {
  id: string
  ownerType: string
  ownerId: string
  name: string
  description: string | null
  defaultBranch: string
  branches: { name: string }[]
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const overview = ref<RepoOverview | null>(null)
const tree = ref<TreeEntry[]>([])
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
      path: currentPath.value
    })
    .toPromise()
  if (result.error) throw result.error
  tree.value = result.data?.repository?.tree ?? []
}

// NOTE: the backend has no file-content resolver (no `repoFileContent`
// query), so README rendering is not currently possible from this API.

async function loadAll() {
  loading.value = true
  error.value = null
  try {
    await loadOverview()
    await loadTree()
  } catch (err: any) {
    error.value = err?.message || 'Failed to load repository'
  } finally {
    loading.value = false
  }
}

function openEntry(entry: TreeEntry) {
  if (entry.kind === 'dir') {
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
            {{ owner }}/{{ overview.name }}
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
            <span>{{ entry.kind === 'dir' ? '📁' : '📄' }}</span>
            {{ entry.name }}
          </li>
        </ul>
      </div>
    </template>
  </div>
</template>
