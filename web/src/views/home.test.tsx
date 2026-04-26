import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { describe, expect, it } from 'vitest'

import { HomeView } from './home'

describe('HomeView', () => {
  it('renders the title', () => {
    render(
      <MemoryRouter>
        <HomeView />
      </MemoryRouter>
    )
    expect(screen.getByText('torajs')).toBeInTheDocument()
  })
})
