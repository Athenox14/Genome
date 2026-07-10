import { useAuthStore } from '~/stores/auth'

// Restore the JWT from localStorage and refresh the current user on app boot.
export default defineNuxtPlugin(async (nuxtApp) => {
  const auth = useAuthStore(nuxtApp.$pinia as any)
  auth.hydrate()
  if (auth.token) {
    await auth.fetchMe()
  }
})
