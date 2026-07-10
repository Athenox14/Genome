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
const isEmptyRepo = ref(false)

const config = useRuntimeConfig()
const cloneUrl = computed(() => `${config.public.apiBase}/${owner.value}/${repoName.value}.git`)

function isMissingRefError(err: any): boolean {
  const msg = String(err?.message || '')
  return /reference .* not found/i.test(msg)
}

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
  isEmptyRepo.value = false
  try {
    await loadOverview()
    try {
      await loadTree()
    } catch (treeErr: any) {
      if (isMissingRefError(treeErr)) {
        // Repo exists but has no commits yet on the default branch.
        isEmptyRepo.value = true
        tree.value = []
      } else {
        throw treeErr
      }
    }
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
    <p v-if="error" class="gh-card mb-4 border-danger bg-red-50 p-3 text-sm text-danger">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <template v-else-if="overview">
      <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 class="text-xl font-semibold text-fg">
            {{ owner }}/{{ overview.name }}
          </h1>
          <p v-if="overview.description" class="text-sm text-fg-muted">
            {{ overview.description }}
          </p>
        </div>

        <select
          v-model="currentRef"
          class="rounded-md border border-border bg-white px-2 py-1 text-sm text-fg"
          @change="onBranchChange"
        >
          <option v-for="b in overview.branches" :key="b.name" :value="b.name">{{ b.name }}</option>
        </select>
      </div>

      <div class="mb-6 flex gap-5 border-b border-border text-sm">
        <NuxtLink :to="`/${owner}/${repoName}`" class="border-b-2 border-fg pb-2.5 font-semibold text-fg">
          Code
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/issues`" class="border-b-2 border-transparent pb-2.5 text-fg-muted hover:text-fg">
          Issues
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/pulls`" class="border-b-2 border-transparent pb-2.5 text-fg-muted hover:text-fg">
          Pull requests
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/actions`" class="border-b-2 border-transparent pb-2.5 text-fg-muted hover:text-fg">
          Actions
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/wiki`" class="border-b-2 border-transparent pb-2.5 text-fg-muted hover:text-fg">
          Wiki
        </NuxtLink>
        <NuxtLink :to="`/${owner}/${repoName}/projects`" class="border-b-2 border-transparent pb-2.5 text-fg-muted hover:text-fg">
          Projects
        </NuxtLink>
      </div>

      <div v-if="isEmptyRepo" class="gh-card p-6">
        <h2 class="mb-2 text-base font-semibold text-fg">This repository is empty</h2>
        <p class="mb-4 text-sm text-fg-muted">
          There isn't any code here yet. Get started by cloning the repository and pushing your first commit.
        </p>
        <div class="rounded-md border border-border bg-canvas-subtle p-3">
          <p class="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">…or create a new repository on the command line</p>
          <pre class="overflow-x-auto rounded-md bg-gray-900 p-3 text-xs text-gray-100"><code>git clone {{ cloneUrl }}
cd {{ overview.name }}
git init
git add .
git commit -m "Initial commit"
git branch -M {{ overview.defaultBranch || 'main' }}
git remote add origin {{ cloneUrl }}
git push -u origin {{ overview.defaultBranch || 'main' }}</code></pre>
        </div>
      </div>

      <div v-else class="gh-card overflow-hidden">
        <div class="flex items-center gap-2 border-b border-border bg-canvas-subtle p-2 text-sm">
          <button v-if="currentPath" class="text-accent hover:underline" @click="goUp">.. (up)</button>
          <span class="font-mono text-fg-muted">/{{ currentPath }}</span>
        </div>
        <ul class="divide-y divide-border">
          <li
            v-for="entry in tree"
            :key="entry.path"
            class="flex cursor-pointer items-center gap-2 px-3 py-2 text-sm hover:bg-canvas-subtle"
            @click="openEntry(entry)"
          >
            <span v-if="entry.kind === 'dir'" class="text-fg-muted">📁</span>
            <span v-else class="text-fg-muted">📄</span>
            <span class="font-mono text-fg">{{ entry.name }}</span>
          </li>
          <li v-if="tree.length === 0" class="px-3 py-4 text-sm text-fg-muted">This folder is empty.</li>
        </ul>
      </div>
    </template>
  </div>
</template>
