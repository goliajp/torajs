import axios from 'axios'

// change baseURL to your API endpoint
export const api = axios.create({
  baseURL: '/api',
  timeout: 10_000,
})
