import { createClient, cacheExchange, fetchExchange, ssrExchange } from '@urql/vue'
import { useAuthStore } from '~/stores/auth'

/**
 * Provides a lightweight urql GraphQL client (`$urql`) for all pages/components.
 * Usage in a page:
 *   const { $urql } = useNuxtApp()
 *   const result = await $urql.query(SOME_QUERY, { vars }).toPromise()
 *
 * Authorization header is pulled from the pinia auth store on every request,
 * so logging in/out automatically affects subsequent GraphQL calls.
 */
export default defineNuxtPlugin((nuxtApp) => {
  const config = useRuntimeConfig()
  const authStore = useAuthStore(nuxtApp.$pinia as any)

  const ssr = ssrExchange({ isClient: import.meta.client })

  const client = createClient({
    url: `${config.public.apiBase}/graphql`,
    exchanges: [cacheExchange, ssr, fetchExchange],
    fetchOptions: () => {
      const token = authStore.token
      return {
        headers: token ? { Authorization: `Bearer ${token}` } : {}
      }
    }
  })

  return {
    provide: {
      urql: client
    }
  }
})
