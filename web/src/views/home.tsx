import { Badge } from '@goliapkg/gds'

export function HomeView() {
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <h1 className="text-fg text-2xl font-bold">torajs</h1>
        <Badge color="info">v{__APP_VERSION__}</Badge>
      </div>
      <p className="text-fg-muted max-w-2xl text-base">Hello from torajs.com.</p>
    </div>
  )
}
