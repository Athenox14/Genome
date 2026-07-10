<script setup lang="ts">
import { useAuthStore } from '~/stores/auth'

const auth = useAuthStore()
const router = useRouter()

const username = ref('')
const password = ref('')

async function onSubmit() {
  const ok = await auth.login(username.value, password.value)
  if (ok) {
    router.push('/')
  }
}
</script>

<template>
  <div class="mx-auto mt-16 max-w-sm rounded-lg border border-gray-200 bg-white p-6 shadow-sm dark:bg-gray-900 dark:border-gray-800">
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">Log in to Genome</h1>

    <form class="space-y-4" @submit.prevent="onSubmit">
      <div>
        <label class="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300">Username</label>
        <input
          v-model="username"
          type="text"
          required
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
      </div>

      <div>
        <label class="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300">Password</label>
        <input
          v-model="password"
          type="password"
          required
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
      </div>

      <p v-if="auth.error" class="text-sm text-red-600">{{ auth.error }}</p>

      <button
        type="submit"
        :disabled="auth.loading"
        class="w-full rounded bg-gray-900 px-3 py-2 text-sm font-medium text-white hover:bg-gray-700 disabled:opacity-50"
      >
        {{ auth.loading ? 'Logging in…' : 'Log in' }}
      </button>
    </form>
  </div>
</template>
