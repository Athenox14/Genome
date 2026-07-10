<script setup lang="ts">
import { ORGANIZATIONS_QUERY, CREATE_ORGANIZATION_MUTATION } from '~/graphql/documents'

interface Organization {
  id: string
  name: string
  description: string | null
  createdAt: string
}

const { $urql } = useNuxtApp()

const organizations = ref<Organization[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

const showForm = ref(false)
const newName = ref('')
const newDescription = ref('')
const creating = ref(false)

async function loadOrganizations() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql.query(ORGANIZATIONS_QUERY, {}, { requestPolicy: 'network-only' }).toPromise()
    if (result.error) throw result.error
    organizations.value = result.data?.organizations ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load organizations'
  } finally {
    loading.value = false
  }
}

async function createOrganization() {
  if (!newName.value.trim()) return
  creating.value = true
  try {
    const result = await $urql
      .mutation(CREATE_ORGANIZATION_MUTATION, { name: newName.value, description: newDescription.value || null })
      .toPromise()
    if (result.error) throw result.error
    showForm.value = false
    newName.value = ''
    newDescription.value = ''
    await loadOrganizations()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create organization'
  } finally {
    creating.value = false
  }
}

onMounted(loadOrganizations)
</script>

<template>
  <div>
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-fg">Organizations</h1>
      <button
        class="gh-btn-primary"
        @click="showForm = !showForm"
      >
        New organization
      </button>
    </div>

    <div v-if="showForm" class="gh-card mb-6 p-4">
      <form class="space-y-3" @submit.prevent="createOrganization">
        <input
          v-model="newName"
          placeholder="Organization name"
          required
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg"
        />
        <textarea
          v-model="newDescription"
          placeholder="Description (optional)"
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg"
        />
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
      <li v-if="organizations.length === 0" class="p-4 text-sm text-fg-muted">No organizations yet.</li>
      <li v-for="org in organizations" :key="org.id" class="p-4">
        <span class="font-medium text-accent hover:underline">{{ org.name }}</span>
        <p v-if="org.description" class="mt-1 text-sm text-fg-muted">{{ org.description }}</p>
      </li>
    </ul>
  </div>
</template>
