<script setup lang="ts">
import {
  DEV_WORKSPACES_QUERY,
  CREATE_DEV_WORKSPACE_MUTATION,
  START_DEV_WORKSPACE_MUTATION,
  STOP_DEV_WORKSPACE_MUTATION
} from '~/graphql/documents'

interface DevWorkspace {
  id: string
  name: string
  repoId: string | null
  image: string
  status: string
  createdAt: string
}

const { $urql } = useNuxtApp()

const workspaces = ref<DevWorkspace[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

const showForm = ref(false)
const newName = ref('')
const newImage = ref('')
const creating = ref(false)
const pendingActionId = ref<string | null>(null)

function statusBadgeClass(status: string) {
  if (status === 'running') return 'bg-green-100 text-green-700'
  if (status === 'starting' || status === 'stopping') return 'bg-yellow-100 text-yellow-700'
  if (status === 'stopped') return 'bg-gray-100 text-gray-700'
  return 'bg-red-100 text-red-700'
}

async function loadWorkspaces() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql.query(DEV_WORKSPACES_QUERY, {}).toPromise()
    if (result.error) throw result.error
    workspaces.value = result.data?.devWorkspaces ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load workspaces'
  } finally {
    loading.value = false
  }
}

async function createWorkspace() {
  if (!newName.value.trim()) return
  creating.value = true
  try {
    const result = await $urql
      .mutation(CREATE_DEV_WORKSPACE_MUTATION, {
        name: newName.value,
        image: newImage.value || null,
        repoId: null
      })
      .toPromise()
    if (result.error) throw result.error
    showForm.value = false
    newName.value = ''
    newImage.value = ''
    await loadWorkspaces()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create workspace'
  } finally {
    creating.value = false
  }
}

async function startWorkspace(id: string) {
  pendingActionId.value = id
  try {
    const result = await $urql.mutation(START_DEV_WORKSPACE_MUTATION, { workspaceId: id }).toPromise()
    if (result.error) throw result.error
    await loadWorkspaces()
  } catch (err: any) {
    error.value = err?.message || 'Failed to start workspace'
  } finally {
    pendingActionId.value = null
  }
}

async function stopWorkspace(id: string) {
  pendingActionId.value = id
  try {
    const result = await $urql.mutation(STOP_DEV_WORKSPACE_MUTATION, { workspaceId: id }).toPromise()
    if (result.error) throw result.error
    await loadWorkspaces()
  } catch (err: any) {
    error.value = err?.message || 'Failed to stop workspace'
  } finally {
    pendingActionId.value = null
  }
}

onMounted(loadWorkspaces)
</script>

<template>
  <div>
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">Dev workspaces</h1>
      <button
        class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
        @click="showForm = !showForm"
      >
        New workspace
      </button>
    </div>

    <div v-if="showForm" class="mb-6 rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800">
      <form class="space-y-3" @submit.prevent="createWorkspace">
        <input
          v-model="newName"
          placeholder="Workspace name"
          required
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <input
          v-model="newImage"
          placeholder="Container image (optional)"
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
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
      <li v-if="workspaces.length === 0" class="p-4 text-sm text-gray-500">No workspaces yet.</li>
      <li v-for="ws in workspaces" :key="ws.id" class="flex items-center justify-between p-4">
        <div>
          <span class="mr-2 rounded px-1.5 py-0.5 text-xs font-medium" :class="statusBadgeClass(ws.status)">
            {{ ws.status }}
          </span>
          <span class="font-medium text-gray-900 dark:text-white">{{ ws.name }}</span>
          <span v-if="ws.repoId" class="ml-2 text-xs text-gray-500">{{ ws.repoId }}</span>
        </div>
        <div class="flex items-center gap-2">
          <button
            v-if="ws.status !== 'running'"
            :disabled="pendingActionId === ws.id"
            class="rounded bg-green-600 px-2 py-1 text-xs text-white hover:bg-green-700 disabled:opacity-50"
            @click="startWorkspace(ws.id)"
          >
            Start
          </button>
          <button
            v-else
            :disabled="pendingActionId === ws.id"
            class="rounded bg-gray-200 px-2 py-1 text-xs text-gray-800 hover:bg-gray-300 disabled:opacity-50"
            @click="stopWorkspace(ws.id)"
          >
            Stop
          </button>
          <NuxtLink
            v-if="ws.status === 'running'"
            :to="`/workspaces/${ws.id}/open`"
            class="rounded bg-blue-600 px-2 py-1 text-xs text-white hover:bg-blue-700"
          >
            Open
          </NuxtLink>
        </div>
      </li>
    </ul>
  </div>
</template>
