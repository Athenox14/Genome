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
const showUserMenu = ref(false)
const searchQuery = ref('')

const unreadCount = computed(() => notifications.value.filter((n) => !n.readAt).length)

const initial = computed(() => (auth.user?.username || '?').charAt(0).toUpperCase())

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
  showUserMenu.value = false
  if (showNotifications.value) loadNotifications()
}

function toggleUserMenu() {
  showUserMenu.value = !showUserMenu.value
  showNotifications.value = false
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
  <header class="border-b border-black/20 bg-canvas-dark text-fg-dark">
    <nav class="mx-auto flex max-w-6xl flex-wrap items-center gap-4 px-4 py-3 md:px-6">
      <NuxtLink to="/" class="flex items-center gap-2 text-white">
        <svg viewBox="0 0 16 16" width="28" height="28" fill="currentColor" aria-hidden="true">
          <path d="M8 0a8 8 0 0 0-2.53 15.59c.4.07.55-.17.55-.38v-1.49c-2.23.48-2.7-1.07-2.7-1.07-.36-.93-.89-1.17-.89-1.17-.72-.5.06-.49.06-.49.8.06 1.22.82 1.22.82.71 1.22 1.87.87 2.33.66.07-.52.28-.87.5-1.07-1.78-.2-3.65-.89-3.65-3.96 0-.87.31-1.59.82-2.15-.08-.2-.36-1.01.08-2.1 0 0 .67-.22 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.09.16 1.9.08 2.1.51.56.82 1.28.82 2.15 0 3.08-1.88 3.75-3.66 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0 0 8 0Z"/>
        </svg>
        <span class="text-lg font-semibold">Genome</span>
      </NuxtLink>

      <div class="flex flex-wrap items-center gap-4 text-sm text-fg-dark/80">
        <NuxtLink to="/" class="hover:text-white">Repositories</NuxtLink>
        <NuxtLink to="/organizations" class="hover:text-white">Organizations</NuxtLink>
        <NuxtLink to="/workspaces" class="hover:text-white">Workspaces</NuxtLink>
        <template v-if="inRepo">
          <NuxtLink :to="`/${owner}/${repoName}/wiki`" class="hover:text-white">Wiki</NuxtLink>
          <NuxtLink :to="`/${owner}/${repoName}/projects`" class="hover:text-white">Projects</NuxtLink>
          <NuxtLink :to="`/${owner}/${repoName}/activity`" class="hover:text-white">Activity</NuxtLink>
        </template>
        <ClientOnly>
          <NuxtLink v-if="auth.user?.isAdmin" to="/admin" class="hover:text-white">Admin</NuxtLink>
        </ClientOnly>
      </div>

      <form class="ml-auto hidden sm:block" @submit.prevent="runSearch">
        <div class="relative">
          <svg class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-dark/50" width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M15.7 14.3 11.5 10a6 6 0 1 0-1.4 1.4l4.3 4.3ZM2 6a4 4 0 1 1 8 0 4 4 0 0 1-8 0Z"/></svg>
          <input
            v-model="searchQuery"
            placeholder="Search…"
            class="w-48 rounded-md border border-white/10 bg-white/5 py-1.5 pl-7 pr-2 text-sm text-white placeholder-fg-dark/50 focus:border-accent-dark focus:outline-none focus:ring-1 focus:ring-accent-dark"
          />
        </div>
      </form>

      <ClientOnly>
        <div class="flex items-center gap-3" :class="{ 'ml-auto': !auth.isAuthenticated }">
          <template v-if="auth.isAuthenticated">
            <div class="relative">
              <button
                class="relative rounded-md p-1.5 text-fg-dark/80 hover:bg-white/10 hover:text-white"
                @click="toggleNotifications"
              >
                🔔
                <span
                  v-if="unreadCount > 0"
                  class="absolute -right-1 -top-1 rounded-full bg-danger-dark px-1 text-xs text-white"
                >
                  {{ unreadCount }}
                </span>
              </button>
              <div
                v-if="showNotifications"
                class="absolute right-0 z-10 mt-2 w-72 rounded-md border border-border-dark bg-canvas-subtle-dark shadow-lg"
              >
                <div class="max-h-80 overflow-y-auto">
                  <p v-if="notifications.length === 0" class="p-3 text-sm text-fg-muted-dark">No notifications.</p>
                  <button
                    v-for="notification in notifications"
                    :key="notification.id"
                    class="block w-full border-b border-border-dark p-3 text-left text-xs last:border-b-0"
                    :class="notification.readAt ? 'text-fg-muted-dark' : 'text-fg-dark'"
                    @click="markRead(notification)"
                  >
                    {{ notification.message }}
                  </button>
                </div>
              </div>
            </div>

            <div class="relative">
              <button
                class="flex h-8 w-8 items-center justify-center rounded-full bg-accent-dark text-sm font-semibold text-white"
                @click="toggleUserMenu"
              >
                {{ initial }}
              </button>
              <div
                v-if="showUserMenu"
                class="absolute right-0 z-10 mt-2 w-48 rounded-md border border-border-dark bg-canvas-subtle-dark py-1 text-sm shadow-lg"
              >
                <div class="border-b border-border-dark px-3 py-2 text-fg-dark">
                  Signed in as <strong>{{ auth.user?.username }}</strong>
                </div>
                <NuxtLink to="/settings/security" class="block px-3 py-1.5 text-fg-dark hover:bg-white/5" @click="showUserMenu = false">
                  Security settings
                </NuxtLink>
                <button class="block w-full px-3 py-1.5 text-left text-fg-dark hover:bg-white/5" @click="handleLogout">
                  Log out
                </button>
              </div>
            </div>
          </template>
          <NuxtLink v-else to="/login" class="gh-btn-primary">
            Log in
          </NuxtLink>
        </div>
        <template #fallback>
          <div class="h-8 w-16"></div>
        </template>
      </ClientOnly>
    </nav>
  </header>
</template>
