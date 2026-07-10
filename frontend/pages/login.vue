<script setup lang="ts">
import { useAuthStore } from '~/stores/auth'

const auth = useAuthStore()
const router = useRouter()

const username = ref('')
const password = ref('')
const totpCode = ref('')

async function onSubmit() {
  const ok = await auth.login(username.value, password.value, totpCode.value || undefined)
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
          :disabled="auth.totpRequired"
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white disabled:opacity-50"
        />
      </div>

      <div>
        <label class="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300">Password</label>
        <input
          v-model="password"
          type="password"
          required
          :disabled="auth.totpRequired"
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white disabled:opacity-50"
        />
      </div>

      <div v-if="auth.totpRequired">
        <label class="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300">
          Two-factor authentication code
        </label>
        <input
          v-model="totpCode"
          type="text"
          inputmode="numeric"
          autocomplete="one-time-code"
          placeholder="123456"
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
        {{ auth.loading ? 'Logging in…' : (auth.totpRequired ? 'Verify code' : 'Log in') }}
      </button>
    </form>
  </div>
</template>
