<script setup lang="ts">
import { useAuthStore } from '~/stores/auth'
import { MY_NOTIFICATIONS_QUERY, MARK_NOTIFICATION_READ_MUTATION } from '~/graphql/documents'

interface Notification {
  id: string
  kind: string
  message: string
  readAt: string | null
  createdAt: string
}

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()
const { $urql } = useNuxtApp()

const owner = computed(() => (route.params.owner ? String(route.params.owner) : null))
const repoName = computed(() => (route.params.repo ? String(route.params.repo) : null))
const inRepo = computed(() => !!owner.value && !!repoName.value)

const notifications = ref<Notification[]>([])
const showNotifications = ref(false)
const searchQuery = ref('')

const unreadCount = computed(() => notifications.value.filter((n) => !n.readAt).length)

async function loadNotifications() {
  if (!auth.isAuthenticated) return
  try {
    const result = await $urql.query(MY_NOTIFICATIONS_QUERY, { unreadOnly: false }).toPromise()
    notifications.value = result.data?.myNotifications ?? []
  } catch {
    // best-effort; ignore
  }
}

async function markRead(notification: Notification) {
  if (notification.readAt) return
  try {
    await $urql.mutation(MARK_NOTIFICATION_READ_MUTATION, { id: notification.id }).toPromise()
    await loadNotifications()
  } catch {
    // best-effort; ignore
  }
}

function toggleNotifications() {
  showNotifications.value = !showNotifications.value
  if (showNotifications.value) loadNotifications()
}

function runSearch() {
  if (!searchQuery.value.trim()) return
  router.push({ path: '/search', query: { q: searchQuery.value } })
}

function handleLogout() {
  auth.logout()
  router.push('/login')
}

watch(() => auth.isAuthenticated, (value) => {
  if (value) loadNotifications()
}, { immediate: true })
</script>

<template>
  <header class="border-b border-gray-200 bg-white dark:bg-gray-900 dark:border-gray-800">
    <nav class="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-3 px-4 py-3">
      <div class="flex flex-wrap items-center gap-6">
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
        <template v-if="inRepo">
          <NuxtLink :to="`/${owner}/${repoName}/wiki`" class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-300">
            Wiki
          </NuxtLink>
          <NuxtLink :to="`/${owner}/${repoName}/projects`" class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-300">
            Projects
          </NuxtLink>
          <NuxtLink :to="`/${owner}/${repoName}/activity`" class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-300">
            Activity
          </NuxtLink>
        </template>
        <NuxtLink
          v-if="auth.user?.isAdmin"
          to="/admin"
          class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-300"
        >
          Admin
        </NuxtLink>
      </div>

      <div class="flex items-center gap-3">
        <form class="hidden sm:block" @submit.prevent="runSearch">
          <input
            v-model="searchQuery"
            placeholder="Search…"
            class="w-40 rounded border border-gray-300 px-2 py-1 text-sm dark:bg-gray-800 dark:border-gray-700 dark:text-white"
          />
        </form>

        <template v-if="auth.isAuthenticated">
          <div class="relative">
            <button
              class="relative rounded bg-gray-100 px-2 py-1.5 text-sm text-gray-700 hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-200"
              @click="toggleNotifications"
            >
              🔔
              <span
                v-if="unreadCount > 0"
                class="absolute -right-1 -top-1 rounded-full bg-red-600 px-1 text-xs text-white"
              >
                {{ unreadCount }}
              </span>
            </button>
            <div
              v-if="showNotifications"
              class="absolute right-0 z-10 mt-2 w-72 rounded border border-gray-200 bg-white shadow-lg dark:bg-gray-900 dark:border-gray-800"
            >
              <div class="max-h-80 overflow-y-auto">
                <p v-if="notifications.length === 0" class="p-3 text-sm text-gray-500">No notifications.</p>
                <button
                  v-for="notification in notifications"
                  :key="notification.id"
                  class="block w-full border-b border-gray-100 p-3 text-left text-xs last:border-b-0 dark:border-gray-800"
                  :class="notification.readAt ? 'text-gray-400' : 'text-gray-900 dark:text-white'"
                  @click="markRead(notification)"
                >
                  {{ notification.message }}
                </button>
              </div>
            </div>
          </div>

          <span class="text-sm text-gray-700 dark:text-gray-300">
            {{ auth.user?.username || 'Signed in' }}
          </span>
          <NuxtLink
            to="/settings/security"
            class="rounded bg-gray-100 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-200"
          >
            Security
          </NuxtLink>
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
