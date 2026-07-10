<script setup lang="ts">
import { WIKI_PAGE_QUERY, WRITE_WIKI_PAGE_MUTATION, REPO_OVERVIEW_QUERY } from '~/graphql/documents'

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

/** Minimal hand-rolled markdown -> HTML converter (headings, bold/italic,
 * links, inline code, fenced code blocks, and paragraphs). Not a full
 * CommonMark implementation, but enough to render wiki pages readably. */
function renderMarkdown(md: string): string {
  const escapeHtml = (s: string) =>
    s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

  const lines = md.split('\n')
  const html: string[] = []
  let inCode = false
  let paragraph: string[] = []

  const flushParagraph = () => {
    if (paragraph.length) {
      let text = escapeHtml(paragraph.join(' '))
      text = text.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
      text = text.replace(/\*(.+?)\*/g, '<em>$1</em>')
      text = text.replace(/`([^`]+)`/g, '<code>$1</code>')
      text = text.replace(/\[(.+?)\]\((.+?)\)/g, '<a href="$2" class="text-blue-600 hover:underline">$1</a>')
      html.push(`<p>${text}</p>`)
      paragraph = []
    }
  }

  for (const rawLine of lines) {
    if (rawLine.trim().startsWith('```')) {
      flushParagraph()
      if (inCode) html.push('</code></pre>')
      else html.push('<pre class="rounded bg-gray-100 p-3 text-xs dark:bg-gray-800"><code>')
      inCode = !inCode
      continue
    }
    if (inCode) {
      html.push(escapeHtml(rawLine) + '\n')
      continue
    }
    const heading = rawLine.match(/^(#{1,6})\s+(.*)$/)
    if (heading) {
      flushParagraph()
      const level = heading[1].length
      html.push(`<h${level} class="font-semibold mt-4 mb-2">${escapeHtml(heading[2])}</h${level}>`)
      continue
    }
    if (rawLine.trim() === '') {
      flushParagraph()
      continue
    }
    paragraph.push(rawLine)
  }
  flushParagraph()
  return html.join('\n')
}

const renderedHtml = computed(() => renderMarkdown(content.value))

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
      <h1 class="text-xl font-semibold text-gray-900 dark:text-white">
        Wiki · {{ isNew ? 'New page' : pageName }}
      </h1>
      <div class="flex gap-2">
        <NuxtLink
          :to="`/${owner}/${repoName}/wiki`"
          class="rounded bg-gray-100 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-200"
        >
          Back to wiki
        </NuxtLink>
        <button
          v-if="!isNew && !editing"
          class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700"
          @click="editing = true"
        >
          Edit
        </button>
      </div>
    </div>

    <p v-if="error" class="mb-4 text-sm text-red-600">{{ error }}</p>
    <p v-if="loading" class="text-sm text-gray-500">Loading…</p>

    <template v-else>
      <p v-if="notFound && !isNew" class="mb-4 text-sm text-gray-500">
        This page doesn't exist yet. Write it below to create it.
      </p>

      <div v-if="editing" class="space-y-3">
        <input
          v-if="isNew"
          v-model="newPageName"
          placeholder="Page name (e.g. Home)"
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <textarea
          v-model="content"
          rows="16"
          placeholder="Markdown content"
          class="w-full rounded border border-gray-300 px-3 py-2 font-mono text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <input
          v-model="message"
          placeholder="Commit message (optional)"
          class="w-full rounded border border-gray-300 px-3 py-2 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
        />
        <div class="flex gap-2">
          <button
            :disabled="saving"
            class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white hover:bg-gray-700 disabled:opacity-50"
            @click="save"
          >
            {{ saving ? 'Saving…' : 'Save' }}
          </button>
          <button
            v-if="!isNew && !notFound"
            class="rounded bg-gray-100 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-200"
            @click="editing = false"
          >
            Cancel
          </button>
        </div>
      </div>

      <div
        v-else
        class="prose prose-sm max-w-none rounded border border-gray-200 bg-white p-4 dark:bg-gray-900 dark:border-gray-800 dark:prose-invert"
        v-html="renderedHtml"
      />
    </template>
  </div>
</template>
