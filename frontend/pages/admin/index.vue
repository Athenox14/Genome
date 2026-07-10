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
    const result = await $urql.query(ADMIN_LIST_USERS_QUERY, { limit: 100, offset: 0 }).toPromise()
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
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">Admin · Users</h1>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <table v-else class="w-full rounded border border-gray-200 bg-white text-sm dark:bg-gray-900 dark:border-gray-800">
      <thead>
        <tr class="border-b border-gray-200 text-left dark:border-gray-800">
          <th class="p-3 font-medium text-gray-500">Username</th>
          <th class="p-3 font-medium text-gray-500">Email</th>
          <th class="p-3 font-medium text-gray-500">Admin</th>
          <th class="p-3 font-medium text-gray-500">Status</th>
          <th class="p-3 font-medium text-gray-500">Actions</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="user in users" :key="user.id" class="border-b border-gray-100 dark:border-gray-800">
          <td class="p-3 text-gray-900 dark:text-white">{{ user.username }}</td>
          <td class="p-3 text-gray-600 dark:text-gray-300">{{ user.email }}</td>
          <td class="p-3">
            <span
              class="rounded px-1.5 py-0.5 text-xs font-medium"
              :class="user.isAdmin ? 'bg-blue-100 text-blue-700' : 'bg-gray-100 text-gray-600'"
            >
              {{ user.isAdmin ? 'Admin' : 'User' }}
            </span>
          </td>
          <td class="p-3">
            <span
              class="rounded px-1.5 py-0.5 text-xs font-medium"
              :class="user.deactivatedAt ? 'bg-red-100 text-red-700' : 'bg-green-100 text-green-700'"
            >
              {{ user.deactivatedAt ? 'Deactivated' : 'Active' }}
            </span>
          </td>
          <td class="p-3">
            <div class="flex gap-2">
              <button
                class="rounded bg-gray-100 px-2 py-1 text-xs text-gray-700 hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-200"
                @click="toggleAdmin(user)"
              >
                {{ user.isAdmin ? 'Revoke admin' : 'Make admin' }}
              </button>
              <button
                class="rounded bg-red-50 px-2 py-1 text-xs text-red-700 hover:bg-red-100 dark:bg-red-900/30 dark:text-red-300"
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
