<script setup lang="ts">
import { useAuthStore } from '~/stores/auth'

const auth = useAuthStore()
const router = useRouter()

function handleLogout() {
  auth.logout()
  router.push('/login')
}
</script>

<template>
  <header class="border-b border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
    <nav class="mx-auto flex max-w-6xl items-center justify-between px-4 py-3">
      <div class="flex items-center gap-6">
        <NuxtLink to="/" class="text-lg font-bold text-gray-900 dark:text-white">
          Genome
        </NuxtLink>
        <NuxtLink to="/" class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-300">
          Repositories
        </NuxtLink>
        <NuxtLink to="/organizations" class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-300">
          Organizations
        </NuxtLink>
        <NuxtLink to="/workspaces" class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-300">
          Workspaces
        </NuxtLink>
      </div>

      <div class="flex items-center gap-4">
        <template v-if="auth.isAuthenticated">
          <span class="text-sm text-gray-700 dark:text-gray-300">
            {{ auth.user?.username || 'Signed in' }}
          </span>
          <button
            class="rounded bg-gray-100 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-200"
            @click="handleLogout"
          >
            Log out
          </button>
        </template>
        <NuxtLink
          v-else
          to="/login"
          class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
        >
          Log in
        </NuxtLink>
      </div>
    </nav>
  </header>
</template>
