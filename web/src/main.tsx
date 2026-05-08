import './index.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createBrowserRouter, Navigate, RouterProvider } from 'react-router'

import { Bench } from './views/bench'
import { Landing } from './views/landing'
import { Playground } from './views/playground'

const router = createBrowserRouter([
  { path: '/', element: <Landing /> },
  { path: '/bench', element: <Bench /> },
  { path: '/playground', element: <Playground /> },
  { path: '*', element: <Navigate replace to="/" /> },
])

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>
)
