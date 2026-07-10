<script setup lang="ts">
import {
  REPO_PROJECTS_QUERY,
  CREATE_PROJECT_MUTATION,
  ADD_PROJECT_COLUMN_MUTATION,
  ADD_CARD_TO_COLUMN_MUTATION,
  ISSUES_QUERY
} from '~/graphql/documents'

interface Card {
  id: string
  position: number
  issueId: string | null
  issue: { id: string; number: number; title: string } | null
}
interface Column {
  id: string
  name: string
  position: number
  cards: Card[]
}
interface Project {
  id: string
  name: string
  columns: Column[]
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const repoId = ref<string | null>(null)
const projects = ref<Project[]>([])
const openIssues = ref<{ id: string; number: number; title: string }[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

const newProjectName = ref('')
const newColumnName: Record<string, string> = reactive({})
const selectedIssue: Record<string, string> = reactive({})

async function load() {
  loading.value = true
  error.value = null
  try {
    const [projResult, issuesResult] = await Promise.all([
      $urql.query(REPO_PROJECTS_QUERY, { owner: owner.value, repo: repoName.value }).toPromise(),
      $urql.query(ISSUES_QUERY, { owner: owner.value, repo: repoName.value }).toPromise()
    ])
    if (projResult.error) throw projResult.error
    repoId.value = projResult.data?.repository?.id ?? null
    projects.value = projResult.data?.repository?.projects ?? []
    openIssues.value = issuesResult.data?.repository?.issues ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load projects'
  } finally {
    loading.value = false
  }
}

async function createProject() {
  if (!newProjectName.value.trim() || !repoId.value) return
  try {
    const result = await $urql
      .mutation(CREATE_PROJECT_MUTATION, { repoId: repoId.value, name: newProjectName.value })
      .toPromise()
    if (result.error) throw result.error
    newProjectName.value = ''
    await load()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create project'
  }
}

async function addColumn(projectId: string) {
  const name = newColumnName[projectId]?.trim()
  if (!name) return
  const project = projects.value.find((p) => p.id === projectId)
  const position = project ? project.columns.length : 0
  try {
    const result = await $urql
      .mutation(ADD_PROJECT_COLUMN_MUTATION, { projectId, name, position })
      .toPromise()
    if (result.error) throw result.error
    newColumnName[projectId] = ''
    await load()
  } catch (err: any) {
    error.value = err?.message || 'Failed to add column'
  }
}

async function addCard(columnId: string) {
  const issueId = selectedIssue[columnId]
  if (!issueId) return
  const column = projects.value.flatMap((p) => p.columns).find((c) => c.id === columnId)
  const position = column ? column.cards.length : 0
  try {
    const result = await $urql
      .mutation(ADD_CARD_TO_COLUMN_MUTATION, { columnId, issueId, position })
      .toPromise()
    if (result.error) throw result.error
    selectedIssue[columnId] = ''
    await load()
  } catch (err: any) {
    error.value = err?.message || 'Failed to add card'
  }
}

onMounted(load)
</script>

<template>
  <div>
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">
      Projects · {{ owner }}/{{ repoName }}
    </h1>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <template v-else>
      <div class="mb-6 flex gap-2">
        <input
          v-model="newProjectName"
          placeholder="New project name"
          class="rounded border border-gray-300 px-3 py-1.5 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
          @keyup.enter="createProject"
        />
        <button
          class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
          @click="createProject"
        >
          Create project
        </button>
      </div>

      <div v-if="projects.length === 0" class="text-sm text-gray-500">No projects yet.</div>

      <div v-for="project in projects" :key="project.id" class="mb-10">
        <h2 class="mb-3 font-medium text-gray-900 dark:text-white">{{ project.name }}</h2>

        <div class="flex gap-4 overflow-x-auto pb-4">
          <div
            v-for="column in project.columns"
            :key="column.id"
            class="w-64 shrink-0 rounded border border-gray-200 bg-gray-50 p-3 dark:bg-gray-800 dark:border-gray-700"
          >
            <h3 class="mb-2 text-sm font-semibold text-gray-900 dark:text-white">{{ column.name }}</h3>

            <div class="space-y-2">
              <div
                v-for="card in column.cards"
                :key="card.id"
                class="rounded border border-gray-200 bg-white p-2 text-xs dark:bg-gray-900 dark:border-gray-700"
              >
                <span v-if="card.issue">#{{ card.issue.number }} {{ card.issue.title }}</span>
                <span v-else class="text-gray-400">(empty card)</span>
              </div>
              <div v-if="column.cards.length === 0" class="text-xs text-gray-400">No cards</div>
            </div>

            <div class="mt-3 flex flex-col gap-1">
              <select
                v-model="selectedIssue[column.id]"
                class="rounded border border-gray-300 px-2 py-1 text-xs dark:bg-gray-800 dark:border-gray-700 dark:text-white"
              >
                <option value="">Add issue…</option>
                <option v-for="issue in openIssues" :key="issue.id" :value="issue.id">
                  #{{ issue.number }} {{ issue.title }}
                </option>
              </select>
              <button
                class="rounded bg-gray-200 px-2 py-1 text-xs text-gray-700 hover:bg-gray-300 dark:bg-gray-700 dark:text-gray-200"
                @click="addCard(column.id)"
              >
                Add card
              </button>
            </div>
          </div>

          <div class="w-64 shrink-0 rounded border border-dashed border-gray-300 p-3 dark:border-gray-600">
            <input
              v-model="newColumnName[project.id]"
              placeholder="New column name"
              class="mb-2 w-full rounded border border-gray-300 px-2 py-1 text-xs dark:bg-gray-800 dark:border-gray-700 dark:text-white"
            />
            <button
              class="w-full rounded bg-gray-200 px-2 py-1 text-xs text-gray-700 hover:bg-gray-300 dark:bg-gray-700 dark:text-gray-200"
              @click="addColumn(project.id)"
            >
              Add column
            </button>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
