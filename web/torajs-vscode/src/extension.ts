// torajs VS Code extension — minimal LSP client. Activates when a
// .ts file is opened, spawns `tr lsp` as a subprocess, wires
// LanguageClient over stdio.
//
// Per RFC 20260505-lsp-server-skeleton.md L-5: ship-able as a
// `.vsix` for local install via `code --install-extension`. Does
// NOT publish to the marketplace — that's a separate decision
// (publisher account + branding + license review).

import * as vscode from 'vscode'
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node'

let client: LanguageClient | undefined

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration('torajs')
  const trPath = config.get<string>('path', 'tr')

  const serverOptions: ServerOptions = {
    command: trPath,
    args: ['lsp'],
    transport: TransportKind.stdio,
  }

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'typescript' }],
    synchronize: {
      // tr lsp doesn't watch config files yet; placeholder for L-6+.
      configurationSection: 'torajs',
    },
  }

  client = new LanguageClient(
    'torajs',
    'torajs Language Server',
    serverOptions,
    clientOptions
  )

  client.start().catch((err) => {
    vscode.window.showErrorMessage(
      `Failed to start tr lsp at "${trPath}": ${err}`
    )
  })

  context.subscriptions.push({
    dispose: () => {
      if (client) {
        client.stop()
      }
    },
  })
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop()
}
