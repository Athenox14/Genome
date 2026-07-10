<script setup lang="ts">
import { DEV_WORKSPACES_QUERY } from '~/graphql/documents'

interface DevWorkspace {
  id: string
  name: string
  status: string
  proxyUrl: string | null
}

const route = useRoute()
const workspaceId = computed(() => String(route.params.id))

const { $urql } = useNuxtApp()
const config = useRuntimeConfig()

const workspace = ref<DevWorkspace | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)

// The backend proxies the code-server instance for a running workspace.
// Best guess: `${apiBase}/workspaces/{id}/proxy/` if the GraphQL type doesn't
// return an explicit proxyUrl.
const iframeSrc = computed(() => {
  if (!workspace.value) return null
  return workspace.value.proxyUrl || `${config.public.apiBase}/workspaces/${workspace.value.id}/proxy/`
})

async function loadWorkspace() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql.query(DEV_WORKSPACES_QUERY, {}).toPromise()
    if (result.error) throw result.error
    const all: DevWorkspace[] = result.data?.devWorkspaces ?? []
    workspace.value = all.find((w) => w.id === workspaceId.value) ?? null
    if (!workspace.value) error.value = 'Workspace not found'
  } catch (err: any) {
    error.value = err?.message || 'Failed to load workspace'
  } finally {
    loading.value = false
  }
}

onMounted(loadWorkspace)
</script>

<template>
  <div class="flex h-[calc(100vh-8rem)] flex-col">
    <div class="mb-2 flex items-center justify-between">
      <NuxtLink to="/workspaces" class="text-sm text-blue-600 hover:underline">← Back to workspaces</NuxtLink>
      <h1 v-if="workspace" class="text-sm font-medium text-gray-700 dark:text-gray-300">
        {{ workspace.name }}
      </h1>
    </div>

    <p v-if="error" class="text-sm text-red-600">{{ error }}</p>
    <p v-else-if="loading" class="text-sm text-gray-500">Loading…</p>

    <iframe
      v-else-if="iframeSrc"
      :src="iframeSrc"
      class="flex-1 rounded border border-gray-200 dark:border-gray-800"
      title="Dev workspace (code-server)"
    />
  </div>
</template>
