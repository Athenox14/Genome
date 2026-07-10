<script setup lang="ts">
import {
  ADMIN_LIST_USERS_QUERY,
  ADMIN_SET_USER_ADMIN_MUTATION,
  ADMIN_DEACTIVATE_USER_MUTATION
} from '~/graphql/documents'

interface AdminUser {
  id: string
  username: string
  email: string
  isAdmin: boolean
  createdAt: string
  deactivatedAt: string | null
}

const { $urql } = useNuxtApp()

const users = ref<AdminUser[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

async function load() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql.query(ADMIN_LIST_USERS_QUERY, { limit: 100, offset: 0 }, { requestPolicy: 'network-only' }).toPromise()
    if (result.error) throw result.error
    users.value = result.data?.adminListUsers ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load users'
  } finally {
    loading.value = false
  }
}

async function toggleAdmin(user: AdminUser) {
  try {
    const result = await $urql
      .mutation(ADMIN_SET_USER_ADMIN_MUTATION, { userId: user.id, isAdmin: !user.isAdmin })
      .toPromise()
    if (result.error) throw result.error
    await load()
  } catch (err: any) {
    error.value = err?.message || 'Failed to update admin status'
  }
}

async function deactivate(user: AdminUser) {
  try {
    const result = await $urql.mutation(ADMIN_DEACTIVATE_USER_MUTATION, { userId: user.id }).toPromise()
    if (result.error) throw result.error
    await load()
  } catch (err: any) {
    error.value = err?.message || 'Failed to deactivate user'
  }
}

onMounted(load)
</script>

<template>
  <div>
    <h1 class="mb-4 text-xl font-semibold text-fg">Admin · Users</h1>

    <p v-if="error" class="mb-4 text-sm text-danger-emphasis">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <table v-else class="gh-card w-full text-sm">
      <thead>
        <tr class="border-b border-border text-left">
          <th class="p-3 font-medium text-fg-muted">Username</th>
          <th class="p-3 font-medium text-fg-muted">Email</th>
          <th class="p-3 font-medium text-fg-muted">Admin</th>
          <th class="p-3 font-medium text-fg-muted">Status</th>
          <th class="p-3 font-medium text-fg-muted">Actions</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="user in users" :key="user.id" class="border-b border-border">
          <td class="p-3 text-fg">{{ user.username }}</td>
          <td class="p-3 text-fg-muted">{{ user.email }}</td>
          <td class="p-3">
            <span
              class="rounded px-1.5 py-0.5 text-xs font-medium"
              :class="user.isAdmin ? 'bg-accent/10 text-accent' : 'bg-canvas-subtle text-fg-muted'"
            >
              {{ user.isAdmin ? 'Admin' : 'User' }}
            </span>
          </td>
          <td class="p-3">
            <span
              class="rounded px-1.5 py-0.5 text-xs font-medium"
              :class="user.deactivatedAt ? 'text-danger-emphasis' : 'text-success-emphasis'"
            >
              {{ user.deactivatedAt ? 'Deactivated' : 'Active' }}
            </span>
          </td>
          <td class="p-3">
            <div class="flex gap-2">
              <button
                class="gh-btn-secondary px-2 py-1 text-xs"
                @click="toggleAdmin(user)"
              >
                {{ user.isAdmin ? 'Revoke admin' : 'Make admin' }}
              </button>
              <button
                class="rounded border border-danger px-2 py-1 text-xs text-danger-emphasis hover:bg-danger/10"
                @click="deactivate(user)"
              >
                Deactivate
              </button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
