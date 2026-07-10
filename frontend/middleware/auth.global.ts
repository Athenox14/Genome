const TOKEN_STORAGE_KEY = 'genome_auth_token'

// Public routes that don't require authentication. There is currently no
// `/register` page in `frontend/pages/`, only `/login`.
const PUBLIC_ROUTES = new Set(['/login'])

export default defineNuxtRouteMiddleware((to) => {
  if (PUBLIC_ROUTES.has(to.path)) return

  // Match the token storage mechanism used by `frontend/stores/auth.ts`
  // (useAuthStore): a plain localStorage key, client-side only.
  if (import.meta.client) {
    const token = localStorage.getItem(TOKEN_STORAGE_KEY)
    if (!token) {
      return navigateTo('/login')
    }
  }
})
