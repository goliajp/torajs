import './index.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { Landing } from './views/landing'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Landing />
  </StrictMode>
)
