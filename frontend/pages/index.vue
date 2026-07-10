<script setup lang="ts">
import { MY_REPOSITORIES_QUERY, CREATE_REPOSITORY_MUTATION } from '~/graphql/documents'

interface Repository {
  id: string
  ownerType: string
  ownerId: string
  ownerLogin: string
  name: string
  description: string | null
  isPrivate: boolean
  defaultBranch: string
  createdAt: string
}

const { $urql } = useNuxtApp()

// RepositoryObject exposes `ownerLogin` (username or org name), resolved
// server-side from the polymorphic ownerType/ownerId pair.
function ownerSlug(repo: Repository): string {
  return repo.ownerLogin || repo.ownerId
}

const repos = ref<Repository[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

const showCreateForm = ref(false)
const newRepoName = ref('')
const newRepoDescription = ref('')
const newRepoPrivate = ref(false)
const creating = ref(false)

async function loadRepos() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql.query(MY_REPOSITORIES_QUERY, {}, { requestPolicy: 'network-only' }).toPromise()
    if (result.error) throw result.error
    repos.value = result.data?.myRepositories ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load repositories'
  } finally {
    loading.value = false
  }
}

async function createRepo() {
  if (!newRepoName.value.trim()) return
  creating.value = true
  try {
    const result = await $urql
      .mutation(CREATE_REPOSITORY_MUTATION, {
        name: newRepoName.value,
        description: newRepoDescription.value || null,
        isPrivate: !!newRepoPrivate.value
      })
      .toPromise()
    if (result.error) throw result.error
    showCreateForm.value = false
    newRepoName.value = ''
    newRepoDescription.value = ''
    newRepoPrivate.value = false
    await loadRepos()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create repository'
  } finally {
    creating.value = false
  }
}

onMounted(loadRepos)
</script>

<template>
  <div>
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-fg">Your repositories</h1>
      <button
        class="gh-btn-primary"
        @click="showCreateForm = !showCreateForm"
      >
        New repository
      </button>
    </div>

    <div
      v-if="showCreateForm"
      class="gh-card mb-6 p-4"
    >
      <form class="space-y-3" @submit.prevent="createRepo">
        <input
          v-model="newRepoName"
          placeholder="Repository name"
          required
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg"
        />
        <textarea
          v-model="newRepoDescription"
          placeholder="Description (optional)"
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg"
        />
        <label class="flex items-center gap-2 text-sm text-fg-muted">
          <input v-model="newRepoPrivate" type="checkbox" />
          Private
        </label>
        <button
          type="submit"
          :disabled="creating"
          class="gh-btn-primary disabled:opacity-50"
        >
          {{ creating ? 'Creating…' : 'Create' }}
        </button>
      </form>
    </div>

    <p v-if="error" class="mb-4 text-sm text-danger-emphasis">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <ul v-else class="gh-card divide-y divide-border">
      <li v-if="repos.length === 0" class="p-4 text-sm text-fg-muted">No repositories yet.</li>
      <li v-for="repo in repos" :key="repo.id" class="flex items-start gap-3 p-4 hover:bg-canvas-subtle">
        <svg class="mt-1 h-4 w-4 shrink-0 text-fg-muted" viewBox="0 0 16 16" fill="currentColor">
          <path d="M2 2.5A2.5 2.5 0 0 1 4.5 0h8.75a.75.75 0 0 1 .75.75v12.5a.75.75 0 0 1-.75.75h-2.5a.75.75 0 0 1 0-1.5h1.75v-2H4.5a1 1 0 0 0-.98 1.19.75.75 0 1 1-1.47.3A2.5 2.5 0 0 1 2 11.5Zm10.5-.5h-8a1 1 0 0 0-1 1v6.708A2.486 2.486 0 0 1 4.5 9h8Z"/>
        </svg>
        <div>
          <NuxtLink :to="`/${ownerSlug(repo)}/${repo.name}`" class="font-medium text-accent hover:underline">
            {{ ownerSlug(repo) }}/{{ repo.name }}
          </NuxtLink>
          <span v-if="repo.isPrivate" class="ml-2 rounded-full border border-border px-1.5 py-0.5 text-xs text-fg-muted">
            Private
          </span>
          <p v-if="repo.description" class="mt-1 text-sm text-fg-muted">{{ repo.description }}</p>
        </div>
      </li>
    </ul>
  </div>
</template>
