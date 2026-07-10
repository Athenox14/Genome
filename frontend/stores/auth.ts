import { defineStore } from 'pinia'

export interface CurrentUser {
  id: string
  username: string
  email?: string | null
  avatarUrl?: string | null
  isAdmin?: boolean
}

const TOKEN_STORAGE_KEY = 'genome_auth_token'

// Best-guess schema (backend team: align field names as needed):
//
// mutation Login($username: String!, $password: String!) {
//   login(username: $username, password: $password) {
//     token
//     user { id username email avatarUrl }
//   }
// }
//
// query Me {
//   me { id username email avatarUrl }
// }
const LOGIN_MUTATION = /* GraphQL */ `
  mutation Login($username: String!, $password: String!, $totpCode: String) {
    login(username: $username, password: $password, totpCode: $totpCode) {
      token
      user {
        id
        username
        email
        avatarUrl
        isAdmin
      }
    }
  }
`

const ME_QUERY = /* GraphQL */ `
  query Me {
    me {
      id
      username
      email
      avatarUrl
      isAdmin
    }
  }
`

export const useAuthStore = defineStore('auth', {
  state: () => ({
    token: null as string | null,
    user: null as CurrentUser | null,
    loading: false,
    error: null as string | null,
    totpRequired: false
  }),

  getters: {
    isAuthenticated: (state) => !!state.token
  },

  actions: {
    /** Load token from localStorage on client startup. */
    hydrate() {
      if (import.meta.client) {
        const stored = localStorage.getItem(TOKEN_STORAGE_KEY)
        if (stored) this.token = stored
      }
    },

    setToken(token: string | null) {
      this.token = token
      if (import.meta.client) {
        if (token) localStorage.setItem(TOKEN_STORAGE_KEY, token)
        else localStorage.removeItem(TOKEN_STORAGE_KEY)
      }
    },

    async login(username: string, password: string, totpCode?: string) {
      this.loading = true
      this.error = null
      this.totpRequired = false
      const config = useRuntimeConfig()
      try {
        const res = await $fetch<{
          data?: { login?: { token: string; user: CurrentUser } }
          errors?: Array<{ message: string }>
        }>(`${config.public.apiBase}/graphql`, {
          method: 'POST',
          body: {
            query: LOGIN_MUTATION,
            variables: { username, password, totpCode: totpCode || null }
          }
        })

        if (res.errors?.length) {
          const message = res.errors[0].message
          if (message === 'totp_required') {
            this.totpRequired = true
            this.error = totpCode ? null : 'Enter your two-factor authentication code'
            return false
          }
          if (message === 'totp_invalid') {
            this.totpRequired = true
            this.error = 'Invalid two-factor authentication code'
            return false
          }
          throw new Error(message)
        }

        const payload = res.data?.login
        if (!payload) throw new Error('Login failed: no data returned')

        this.setToken(payload.token)
        this.user = payload.user
        return true
      } catch (err: any) {
        this.error = err?.data?.message || err?.message || 'Login failed'
        return false
      } finally {
        this.loading = false
      }
    },

    async fetchMe() {
      if (!this.token) return null
      const config = useRuntimeConfig()
      try {
        const res = await $fetch<{
          data?: { me?: CurrentUser }
          errors?: Array<{ message: string }>
        }>(`${config.public.apiBase}/graphql`, {
          method: 'POST',
          headers: { Authorization: `Bearer ${this.token}` },
          body: { query: ME_QUERY }
        })
        if (res.errors?.length) throw new Error(res.errors[0].message)
        this.user = res.data?.me || null
        return this.user
      } catch (err) {
        // Token likely invalid/expired.
        this.logout()
        return null
      }
    },

    logout() {
      this.setToken(null)
      this.user = null
    }
  }
})
