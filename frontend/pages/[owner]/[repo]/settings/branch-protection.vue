<script setup lang="ts">
import {
  REPO_BRANCH_PROTECTION_RULES_QUERY,
  CREATE_BRANCH_PROTECTION_RULE_MUTATION,
  REPO_OVERVIEW_QUERY
} from '~/graphql/documents'

interface Rule {
  id: string
  branchPattern: string
  requireReviewsCount: number
  blockForcePush: boolean
}

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))

const { $urql } = useNuxtApp()

const repoId = ref<string | null>(null)
const rules = ref<Rule[]>([])
const loading = ref(true)
const error = ref<string | null>(null)
const submitting = ref(false)

const branchPattern = ref('main')
const requireReviewsCount = ref(0)
const blockForcePush = ref(true)

async function load() {
  loading.value = true
  error.value = null
  try {
    const [repoResult, rulesResult] = await Promise.all([
      $urql.query(REPO_OVERVIEW_QUERY, { owner: owner.value, repo: repoName.value }).toPromise(),
      $urql.query(REPO_BRANCH_PROTECTION_RULES_QUERY, { owner: owner.value, repo: repoName.value }).toPromise()
    ])
    if (rulesResult.error) throw rulesResult.error
    repoId.value = repoResult.data?.repository?.id ?? null
    rules.value = rulesResult.data?.repository?.branchProtectionRules ?? []
  } catch (err: any) {
    error.value = err?.message || 'Failed to load branch protection rules'
  } finally {
    loading.value = false
  }
}

async function createRule() {
  if (!repoId.value || !branchPattern.value.trim()) return
  submitting.value = true
  error.value = null
  try {
    const result = await $urql
      .mutation(CREATE_BRANCH_PROTECTION_RULE_MUTATION, {
        repoId: repoId.value,
        branchPattern: branchPattern.value,
        requireReviewsCount: requireReviewsCount.value,
        blockForcePush: blockForcePush.value
      })
      .toPromise()
    if (result.error) throw result.error
    branchPattern.value = 'main'
    requireReviewsCount.value = 0
    blockForcePush.value = true
    await load()
  } catch (err: any) {
    error.value = err?.message || 'Failed to create branch protection rule'
  } finally {
    submitting.value = false
  }
}

onMounted(load)
</script>

<template>
  <div class="mx-auto max-w-xl">
    <h1 class="mb-4 text-xl font-semibold text-gray-900 dark:text-white">
      Branch protection · {{ owner }}/{{ repoName }}
    </h1>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <template v-else>
      <ul class="mb-6 divide-y divide-gray-200 rounded border border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
        <li v-if="rules.length === 0" class="p-4 text-sm text-gray-500">No rules configured.</li>
        <li v-for="rule in rules" :key="rule.id" class="p-4 text-sm">
          <span class="font-medium text-gray-900 dark:text-white">{{ rule.branchPattern }}</span>
          <span class="ml-2 text-gray-500">
            requires {{ rule.requireReviewsCount }} review(s)
            <template v-if="rule.blockForcePush">· force-push blocked</template>
          </span>
        </li>
      </ul>

      <form class="space-y-3 rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800" @submit.prevent="createRule">
        <div>
          <label class="mb-1 block text-xs text-gray-500">Branch pattern</label>
          <input
            v-model="branchPattern"
            placeholder="main or release/*"
            class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
          />
        </div>
        <div>
          <label class="mb-1 block text-xs text-gray-500">Required approving reviews</label>
          <input
            v-model.number="requireReviewsCount"
            type="number"
            min="0"
            class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
          />
        </div>
        <label class="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300">
          <input v-model="blockForcePush" type="checkbox" />
          Block force pushes
        </label>
        <button
          type="submit"
          :disabled="submitting"
          class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700 disabled:opacity-50"
        >
          {{ submitting ? 'Creating…' : 'Create rule' }}
        </button>
      </form>
    </template>
  </div>
</template>
