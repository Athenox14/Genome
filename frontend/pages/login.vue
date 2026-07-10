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
  <div class="gh-card mx-auto mt-16 max-w-sm p-6">
    <h1 class="mb-4 text-xl font-semibold text-fg">Log in to Genome</h1>

    <form class="space-y-4" @submit.prevent="onSubmit">
      <div>
        <label class="mb-1 block text-sm font-medium text-fg-muted">Username</label>
        <input
          v-model="username"
          type="text"
          required
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg disabled:opacity-50"
        />
      </div>

      <div>
        <label class="mb-1 block text-sm font-medium text-fg-muted">Password</label>
        <input
          v-model="password"
          type="password"
          required
          class="w-full rounded border border-border px-3 py-2 text-sm text-fg disabled:opacity-50"
        />
      </div>

      <p v-if="auth.error" class="text-sm text-danger-emphasis">{{ auth.error }}</p>

      <button
        type="submit"
        :disabled="auth.loading"
        class="gh-btn-primary w-full disabled:opacity-50"
      >
        {{ auth.loading ? 'Logging in…' : 'Log in' }}
      </button>
    </form>
  </div>
</template>
