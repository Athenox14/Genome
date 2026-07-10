<script setup lang="ts">
import { ORGANIZATIONS_QUERY, CREATE_ORGANIZATION_MUTATION } from '~/graphql/documents'

interface Organization {
  id: string
  name: string
  description: string | null
  memberCount: number
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
    const result = await $urql.query(ORGANIZATIONS_QUERY, {}).toPromise()
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
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">Organizations</h1>
      <button
        class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
        @click="showForm = !showForm"
      >
        New organization
      </button>
    </div>

    <div v-if="showForm" class="mb-6 rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800">
      <form class="space-y-3" @submit.prevent="createOrganization">
        <input
          v-model="newName"
          placeholder="Organization name"
          required
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <textarea
          v-model="newDescription"
          placeholder="Description (optional)"
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
      <li v-if="organizations.length === 0" class="p-4 text-sm text-gray-500">No organizations yet.</li>
      <li v-for="org in organizations" :key="org.id" class="p-4">
        <span class="font-medium text-gray-900 dark:text-white">{{ org.name }}</span>
        <span class="ml-2 text-xs text-gray-500">{{ org.memberCount }} members</span>
        <p v-if="org.description" class="mt-1 text-sm text-gray-600 dark:text-gray-400">{{ org.description }}</p>
      </li>
    </ul>
  </div>
</template>
