<script setup lang="ts">
import { ISSUES_QUERY, CREATE_ISSUE_MUTATION } from '~/graphql/documents'

interface Issue {
  id: string
  number: number
  title: string
  state: string
  author: { username: string } | null
  createdAt: string
  commentCount: number
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const issues = ref<Issue[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

const showForm = ref(false)
const title = ref('')
const body = ref('')
const submitting = ref(false)

async function loadIssues() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql
      .query(ISSUES_QUERY, { owner: owner.value, repo: repoName.value })
      .toPromise()
    if (result.error) throw result.error
    issues.value = result.data?.issues ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load issues'
  } finally {
    loading.value = false
  }
}

async function createIssue() {
  if (!title.value.trim()) return
  submitting.value = true
  try {
    const result = await $urql
      .mutation(CREATE_ISSUE_MUTATION, {
        owner: owner.value,
        repo: repoName.value,
        title: title.value,
        body: body.value || null
      })
      .toPromise()
    if (result.error) throw result.error
    title.value = ''
    body.value = ''
    showForm.value = false
    await loadIssues()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create issue'
  } finally {
    submitting.value = false
  }
}

onMounted(loadIssues)
</script>

<template>
  <div>
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">
        Issues · {{ owner }}/{{ repoName }}
      </h1>
      <button
        class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
        @click="showForm = !showForm"
      >
        New issue
      </button>
    </div>

    <div v-if="showForm" class="mb-6 rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800">
      <form class="space-y-3" @submit.prevent="createIssue">
        <input
          v-model="title"
          placeholder="Issue title"
          required
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <textarea
          v-model="body"
          placeholder="Describe the issue"
          rows="4"
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <button
          type="submit"
          :disabled="submitting"
          class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700 disabled:opacity-50"
        >
          {{ submitting ? 'Submitting…' : 'Submit' }}
        </button>
      </form>
    </div>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <ul v-else class="divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
      <li v-if="issues.length === 0" class="p-4 text-sm text-gray-500">No issues yet.</li>
      <li v-for="issue in issues" :key="issue.id" class="flex items-center justify-between p-4">
        <div>
          <span
            class="mr-2 rounded px-1.5 py-0.5 text-xs font-medium"
            :class="issue.state === 'open' ? 'bg-green-100 text-green-700' : 'bg-purple-100 text-purple-700'"
          >
            {{ issue.state }}
          </span>
          <span class="font-medium text-gray-900 dark:text-white">#{{ issue.number }} {{ issue.title }}</span>
          <p class="text-xs text-gray-500">
            opened by {{ issue.author?.username || 'unknown' }}
          </p>
        </div>
        <span class="text-xs text-gray-500">{{ issue.commentCount }} comments</span>
      </li>
    </ul>
  </div>
</template>
