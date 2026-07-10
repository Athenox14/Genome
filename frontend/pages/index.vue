<script setup lang="ts">
import { MY_REPOSITORIES_QUERY, CREATE_REPOSITORY_MUTATION } from '~/graphql/documents'

interface Repository {
  id: string
  name: string
  owner: string
  description: string | null
  isPrivate: boolean
  defaultBranch: string
  updatedAt: string
  starCount: number
}

const { $urql } = useNuxtApp()

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
    const result = await $urql.query(MY_REPOSITORIES_QUERY, {}).toPromise()
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
        isPrivate: newRepoPrivate.value
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
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">Your repositories</h1>
      <button
        class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
        @click="showCreateForm = !showCreateForm"
      >
        New repository
      </button>
    </div>

    <div
      v-if="showCreateForm"
      class="mb-6 rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800"
    >
      <form class="space-y-3" @submit.prevent="createRepo">
        <input
          v-model="newRepoName"
          placeholder="Repository name"
          required
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <textarea
          v-model="newRepoDescription"
          placeholder="Description (optional)"
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <label class="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300">
          <input v-model="newRepoPrivate" type="checkbox" />
          Private
        </label>
        <button
          type="submit"
          :disabled="creating"
          class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700 disabled:opacity-50"
        >
          {{ creating ? 'Creating…' : 'Create' }}
        </button>
      </form>
    </div>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <ul v-else class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
      <li v-if="repos.length === 0" class="p-4 text-sm text-gray-500">No repositories yet.</li>
      <li v-for="repo in repos" :key="repo.id" class="p-4 hover:bg-gray-50 dark:hover:bg-gray-800">
        <NuxtLink :to="`/${repo.owner}/${repo.name}`" class="font-medium text-blue-600 hover:underline">
          {{ repo.owner }}/{{ repo.name }}
        </NuxtLink>
        <span v-if="repo.isPrivate" class="ml-2 rounded bg-gray-200 px-1.5 py-0.5 text-xs text-gray-700 dark:bg-gray-700 dark:text-gray-200">
          Private
        </span>
        <p v-if="repo.description" class="mt-1 text-sm text-gray-600 dark:text-gray-400">{{ repo.description }}</p>
      </li>
    </ul>
  </div>
</template>
