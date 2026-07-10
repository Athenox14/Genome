import { useAuthStore } from '~/stores/auth'

export default defineNuxtPlugin(async (nuxtApp) => {
  const auth = useAuthStore(nuxtApp.$pinia as any)
  auth.hydrate()
  if (auth.token) {
    await auth.fetchMe()
  }
})
