// https://nuxt.com/docs/api/configuration/nuxt-config
export default defineNuxtConfig({
  compatibilityDate: '2024-10-01',
  devtools: { enabled: true },

  modules: ['@nuxtjs/tailwindcss', '@pinia/nuxt'],

  css: ['~/assets/css/main.css'],

  runtimeConfig: {
    public: {
      // Base URL of the Genome backend (GraphQL at /graphql, git smart-http at /{owner}/{repo}.git)
      apiBase: process.env.NUXT_PUBLIC_API_BASE || 'http://localhost:8000'
    }
  },

  typescript: {
    strict: false,
    shim: false
  }
})
