<script setup lang="ts">
import { DEV_WORKSPACES_QUERY } from '~/graphql/documents'

interface DevWorkspace {
  id: string
  name: string
  status: string
}

const route = useRoute()
const workspaceId = computed(() => String(route.params.id))

const { $urql } = useNuxtApp()
const config = useRuntimeConfig()

const workspace = ref<DevWorkspace | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)

// DevWorkspaceObject has no `proxyUrl` field on the backend; the code-server
// instance is reached through the API's workspace proxy route directly.
const iframeSrc = computed(() => {
  if (!workspace.value) return null
  return `${config.public.apiBase}/workspaces/${workspace.value.id}/proxy/`
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
      <NuxtLink to="/workspaces" class="text-sm text-accent hover:underline">← Back to workspaces</NuxtLink>
      <h1 v-if="workspace" class="text-sm font-medium text-fg-muted">
        {{ workspace.name }}
      </h1>
    </div>

    <p v-if="error" class="text-sm text-danger-emphasis">{{ error }}</p>
    <p v-else-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <iframe
      v-else-if="iframeSrc"
      :src="iframeSrc"
      class="flex-1 rounded border border-border"
      title="Dev workspace (code-server)"
    />
  </div>
</template>
