<script setup lang="ts">
import { WIKI_PAGE_QUERY, WRITE_WIKI_PAGE_MUTATION, REPO_OVERVIEW_QUERY } from '~/graphql/documents'
import { marked } from 'marked'

const route = useRoute()
const owner = computed(() => String(route.params.owner))
const repoName = computed(() => String(route.params.repo))
const pageName = computed(() => String(route.params.page))
const isNew = computed(() => pageName.value === 'new')

const { $urql } = useNuxtApp()

const content = ref('')
const newPageName = ref('')
const message = ref('')
const repoId = ref<string | null>(null)
const loading = ref(true)
const saving = ref(false)
const editing = ref(false)
const error = ref<string | null>(null)
const notFound = ref(false)

// Full CommonMark rendering via `marked` (tables, lists, etc. included).
// NOTE: like the previous hand-rolled renderer's output, this is injected
// via `v-html` without HTML sanitization. Unlike the old renderer — which
// HTML-escaped the source text before applying its own limited formatting,
// so raw HTML in wiki content could never execute — `marked` passes through
// raw HTML embedded in the markdown source unescaped. Wiki content is
// treated here as trusted (same trust level as before for repo
// collaborators), but this is a lateral behavior change worth hardening
// with an HTML sanitizer (e.g. DOMPurify) if wiki content should ever be
// treated as untrusted input.
const renderedHtml = computed(() => marked.parse(content.value, { async: false }) as string)

async function loadRepoId() {
  const result = await $urql
    .query(REPO_OVERVIEW_QUERY, { owner: owner.value, repo: repoName.value })
    .toPromise()
  repoId.value = result.data?.repository?.id ?? null
}

async function loadPage() {
  loading.value = true
  error.value = null
  notFound.value = false
  try {
    await loadRepoId()
    if (isNew.value) {
      editing.value = true
      content.value = ''
      loading.value = false
      return
    }
    const result = await $urql
      .query(WIKI_PAGE_QUERY, { owner: owner.value, repo: repoName.value, page: pageName.value })
      .toPromise()
    if (result.error) throw result.error
    const page = result.data?.repository?.wikiPage
    if (page == null) {
      notFound.value = true
      editing.value = true
    } else {
      content.value = page
    }
  } catch (err: any) {
    error.value = err?.message || 'Failed to load wiki page'
  } finally {
    loading.value = false
  }
}

async function save() {
  if (!repoId.value) return
  const targetPage = isNew.value ? newPageName.value.trim() : pageName.value
  if (!targetPage) {
    error.value = 'Page name is required'
    return
  }
  saving.value = true
  error.value = null
  try {
    const result = await $urql
      .mutation(WRITE_WIKI_PAGE_MUTATION, {
        repoId: repoId.value,
        page: targetPage,
        content: content.value,
        message: message.value || `Update ${targetPage}`
      })
      .toPromise()
    if (result.error) throw result.error
    message.value = ''
    if (isNew.value) {
      await navigateTo(`/${owner.value}/${repoName.value}/wiki/${encodeURIComponent(targetPage)}`)
    } else {
      editing.value = false
      notFound.value = false
    }
  } catch (err: any) {
    error.value = err?.message || 'Failed to save wiki page'
  } finally {
    saving.value = false
  }
}

onMounted(loadPage)
</script>

<template>
  <div>
    <div class="mb-4 flex items-center justify-between">
      <h1 class="text-xl font-semibold text-fg">
        Wiki · {{ isNew ? 'New page' : pageName }}
      </h1>
      <div class="flex gap-2">
        <NuxtLink
          :to="`/${owner}/${repoName}/wiki`"
          class="gh-btn-secondary"
        >
          Back to wiki
        </NuxtLink>
        <button
          v-if="!isNew && !editing"
          class="gh-btn-primary"
          @click="editing = true"
        >
          Edit
        </button>
      </div>
    </div>

    <p v-if="error" class="gh-card mb-4 border-danger bg-red-50 p-3 text-sm text-danger">{{ error }}</p>
    <p v-if="loading" class="text-sm text-fg-muted">Loading…</p>

    <template v-else>
      <p v-if="notFound && !isNew" class="mb-4 text-sm text-fg-muted">
        This page doesn't exist yet. Write it below to create it.
      </p>

      <div v-if="editing" class="space-y-3">
        <input
          v-if="isNew"
          v-model="newPageName"
          placeholder="Page name (e.g. Home)"
          class="w-full rounded-md border border-border px-3 py-2 text-sm text-fg"
        />
        <textarea
          v-model="content"
          rows="16"
          placeholder="Markdown content"
          class="w-full rounded-md border border-border px-3 py-2 font-mono text-sm text-fg"
        />
        <input
          v-model="message"
          placeholder="Commit message (optional)"
          class="w-full rounded-md border border-border px-3 py-2 text-sm text-fg"
        />
        <div class="flex gap-2">
          <button
            :disabled="saving"
            class="gh-btn-primary disabled:opacity-50"
            @click="save"
          >
            {{ saving ? 'Saving…' : 'Save' }}
          </button>
          <button
            v-if="!isNew && !notFound"
            class="gh-btn-secondary"
            @click="editing = false"
          >
            Cancel
          </button>
        </div>
      </div>

      <div
        v-else
        class="gh-card prose prose-sm max-w-none p-4 text-fg"
        v-html="renderedHtml"
      />
    </template>
  </div>
</template>
