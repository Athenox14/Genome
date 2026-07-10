<script setup lang="ts">
import {
  ISSUES_WITH_META_QUERY,
  CREATE_ISSUE_MUTATION,
  SET_ISSUE_MILESTONE_MUTATION,
  CREATE_LABEL_MUTATION,
  ADD_LABEL_TO_ISSUE_MUTATION
} from '~/graphql/documents'

interface Label {
  id: string
  name: string
  color: string
}
interface Milestone {
  id: string
  title: string
}
interface Issue {
  id: string
  number: number
  title: string
  state: string
  authorId: string
  createdAt: string
  labels: Label[]
  milestone: Milestone | null
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const issues = ref<Issue[]>([])
const labels = ref<Label[]>([])
const milestones = ref<Milestone[]>([])
const repoId = ref<string | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)

const showForm = ref(false)
const title = ref('')
const body = ref('')
const milestoneId = ref('')
const submitting = ref(false)

const showLabelForm = ref(false)
const labelName = ref('')
const labelColor = ref('#2563eb')

function labelStyle(color: string) {
  return { backgroundColor: color, color: '#fff' }
}

async function loadIssues() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql
      .query(ISSUES_WITH_META_QUERY, { owner: owner.value, repo: repoName.value }, { requestPolicy: 'network-only' })
      .toPromise()
    if (result.error) throw result.error
    repoId.value = result.data?.repository?.id ?? null
    issues.value = result.data?.repository?.issues ?? []
    labels.value = result.data?.repository?.labels ?? []
    milestones.value = result.data?.repository?.milestones ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load issues'
  } finally {
    loading.value = false
  }
}

async function createIssue() {
  if (!title.value.trim() || !repoId.value) return
  submitting.value = true
  try {
    const result = await $urql
      .mutation(CREATE_ISSUE_MUTATION, {
        repoId: repoId.value,
        title: title.value,
        body: body.value || null
      })
      .toPromise()
    if (result.error) throw result.error
    const newIssueId = result.data?.createIssue?.id
    if (newIssueId && milestoneId.value) {
      await $urql
        .mutation(SET_ISSUE_MILESTONE_MUTATION, { issueId: newIssueId, milestoneId: milestoneId.value })
        .toPromise()
    }
    title.value = ''
    body.value = ''
    milestoneId.value = ''
    showForm.value = false
    await loadIssues()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create issue'
  } finally {
    submitting.value = false
  }
}

async function createLabel() {
  if (!labelName.value.trim() || !repoId.value) return
  try {
    const result = await $urql
      .mutation(CREATE_LABEL_MUTATION, { repoId: repoId.value, name: labelName.value, color: labelColor.value })
      .toPromise()
    if (result.error) throw result.error
    labelName.value = ''
    showLabelForm.value = false
    await loadIssues()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create label'
  }
}

async function addLabel(issueId: string, labelId: string) {
  if (!labelId) return
  try {
    const result = await $urql.mutation(ADD_LABEL_TO_ISSUE_MUTATION, { issueId, labelId }).toPromise()
    if (result.error) throw result.error
    await loadIssues()
  } catch (err: any) {
    error.value = err?.message || 'Failed to add label'
  }
}

onMounted(loadIssues)
</script>

<template>
  <div>
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-fg">
        Issues · {{ owner }}/{{ repoName }}
      </h1>
      <div class="flex gap-2">
        <button
          class="gh-btn-secondary"
          @click="showLabelForm = !showLabelForm"
        >
          New label
        </button>
        <button
          class="gh-btn-primary"
          @click="showForm = !showForm"
        >
          New issue
        </button>
      </div>
    </div>

    <div v-if="showLabelForm" class="gh-card mb-6 flex items-end gap-2 p-4">
      <div>
        <label class="mb-1 block text-xs text-fg-muted">Name</label>
        <input v-model="labelName" class="rounded border border-border px-2 py-1 text-sm text-fg" />
      </div>
      <div>
        <label class="mb-1 block text-xs text-fg-muted">Color</label>
        <input v-model="labelColor" type="color" class="h-8 w-12 rounded border border-border" />
      </div>
      <button class="gh-btn-primary" @click="createLabel">
        Add label
      </button>
    </div>

    <div v-if="showForm" class="gh-card mb-6 p-4">
      <form class="space-y-3" @submit.prevent="createIssue">
        <input
          v-model="title"
          placeholder="Issue title"
          required
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg"
        />
        <textarea
          v-model="body"
          placeholder="Describe the issue"
          rows="4"
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg"
        />
        <div>
          <label class="mb-1 block text-xs text-fg-muted">Milestone</label>
          <select
            v-model="milestoneId"
            class="w-full rounded border border-border px-3 py-2 text-sm text-fg"
          >
            <option value="">None</option>
            <option v-for="m in milestones" :key="m.id" :value="m.id">{{ m.title }}</option>
          </select>
        </div>
        <button
          type="submit"
          :disabled="submitting"
          class="gh-btn-primary disabled:opacity-50"
        >
          {{ submitting ? 'Submitting…' : 'Submit' }}
        </button>
      </form>
    </div>

    <p v-if="error" class="mb-4 text-sm text-danger-emphasis">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <ul v-else class="gh-card divide-y divide-border">
      <li v-if="issues.length === 0" class="p-4 text-sm text-fg-muted">No issues yet.</li>
      <li v-for="issue in issues" :key="issue.id" class="p-4">
        <div class="flex items-center justify-between">
          <div class="flex flex-wrap items-center gap-2">
            <svg
              class="h-3 w-3 shrink-0"
              :class="issue.state === 'open' ? 'text-success' : 'text-done'"
              viewBox="0 0 16 16"
              fill="currentColor"
            >
              <circle cx="8" cy="8" r="8" />
            </svg>
            <span class="font-medium text-accent hover:underline">
              #{{ issue.number }} {{ issue.title }}
            </span>
            <span v-if="issue.milestone" class="rounded bg-canvas-subtle px-1.5 py-0.5 text-xs text-fg-muted">
              🎯 {{ issue.milestone.title }}
            </span>
            <span
              v-for="label in issue.labels"
              :key="label.id"
              class="rounded-full px-1.5 py-0.5 text-xs font-medium"
              :style="labelStyle(label.color)"
            >
              {{ label.name }}
            </span>
          </div>
          <select
            class="rounded border border-border px-2 py-1 text-xs text-fg"
            @change="addLabel(issue.id, ($event.target as HTMLSelectElement).value)"
          >
            <option value="">Add label…</option>
            <option v-for="label in labels" :key="label.id" :value="label.id">{{ label.name }}</option>
          </select>
        </div>
        <p class="mt-1 pl-5 text-xs text-fg-muted">
          #{{ issue.number }} · opened {{ new Date(issue.createdAt).toLocaleDateString() }}
        </p>
      </li>
    </ul>
  </div>
</template>
