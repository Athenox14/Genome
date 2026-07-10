<script setup lang="ts">
import {
  ENABLE_TWO_FACTOR_MUTATION,
  CONFIRM_TWO_FACTOR_MUTATION,
  DISABLE_TWO_FACTOR_MUTATION
} from '~/graphql/documents'

const { $urql } = useNuxtApp()

const secret = ref<string | null>(null)
const provisioningUri = ref<string | null>(null)
const code = ref('')
const confirmed = ref(false)
const error = ref<string | null>(null)
const loading = ref(false)

async function enable() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql.mutation(ENABLE_TWO_FACTOR_MUTATION, {}).toPromise()
    if (result.error) throw result.error
    secret.value = result.data?.enableTwoFactor?.secret ?? null
    provisioningUri.value = result.data?.enableTwoFactor?.provisioningUri ?? null
    confirmed.value = false
  } catch (err: any) {
    error.value = err?.message || 'Failed to start two-factor setup'
  } finally {
    loading.value = false
  }
}

async function confirm() {
  if (!code.value.trim()) return
  loading.value = true
  error.value = null
  try {
    const result = await $urql.mutation(CONFIRM_TWO_FACTOR_MUTATION, { code: code.value }).toPromise()
    if (result.error) throw result.error
    confirmed.value = true
    code.value = ''
  } catch (err: any) {
    error.value = err?.message || 'Invalid code'
  } finally {
    loading.value = false
  }
}

async function disable() {
  loading.value = true
  error.value = null
  try {
    const result = await $urql.mutation(DISABLE_TWO_FACTOR_MUTATION, {}).toPromise()
    if (result.error) throw result.error
    secret.value = null
    provisioningUri.value = null
    confirmed.value = false
  } catch (err: any) {
    error.value = err?.message || 'Failed to disable two-factor authentication'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-xl">
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">Security</h1>

    <div class="rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800">
      <h2 class="mb-2 font-medium text-gray-900 dark:text-white">Two-factor authentication</h2>

      <p v-if="error" class="mb-3 text-sm text-red-600">{{ error }}</p>

      <div v-if="confirmed" class="text-sm text-green-700 dark:text-green-400">
        Two-factor authentication is now enabled.
      </div>

      <template v-else-if="provisioningUri">
        <p class="mb-2 text-sm text-gray-600 dark:text-gray-300">
          Scan this URI with your authenticator app, or enter the secret manually, then confirm with a code.
        </p>
        <p class="mb-2 break-all rounded bg-gray-100 p-2 font-mono text-xs dark:bg-gray-800 dark:text-gray-200">
          {{ provisioningUri }}
        </p>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-300">
          Manual entry secret: <span class="font-mono">{{ secret }}</span>
        </p>
        <div class="flex gap-2">
          <input
            v-model="code"
            placeholder="6-digit code"
            class="rounded border border-gray-300 px-3 py-1.5 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
          />
          <button
            :disabled="loading"
            class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700 disabled:opacity-50"
            @click="confirm"
          >
            Confirm
          </button>
        </div>
      </template>

      <template v-else>
        <button
          :disabled="loading"
          class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700 disabled:opacity-50"
          @click="enable"
        >
          Enable 2FA
        </button>
      </template>

      <div class="mt-6 border-t border-gray-200 pt-4 dark:border-gray-800">
        <button
          :disabled="loading"
          class="rounded bg-red-50 px-3 py-1.5 text-sm text-red-700 hover:bg-red-100 disabled:opacity-50 dark:bg-red-900/30 dark:text-red-300"
          @click="disable"
        >
          Disable 2FA
        </button>
      </div>
    </div>
  </div>
</template>
