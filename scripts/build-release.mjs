#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { cp, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const manifest = JSON.parse(await readFile(path.join(root, 'package/manifest.json'), 'utf8'))
const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
const uiPackageJson = JSON.parse(await readFile(path.join(root, 'ui/package.json'), 'utf8'))
const cargoToml = await readFile(path.join(root, 'Cargo.toml'), 'utf8')
const cargoPackageName = cargoToml.match(/^name\s*=\s*"([a-z0-9_-]+)"/m)?.[1]
const workspaceVersion = cargoToml.match(/^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1]

if (!cargoPackageName) throw new Error('Cannot read the package name from Cargo.toml.')
if (manifest.id !== 'knowledge_library') throw new Error('manifest.json has an unexpected plugin ID.')
if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$/.test(manifest.version ?? '')) {
  throw new Error('manifest.json has an invalid plugin version.')
}
if ([packageJson.version, uiPackageJson.version, workspaceVersion].some((version) => version !== manifest.version)) {
  throw new Error('Manifest, root package, UI package, and Rust workspace versions must match.')
}
if (process.env.GITHUB_REF_TYPE === 'tag' && process.env.GITHUB_REF_NAME !== `v${manifest.version}`) {
  throw new Error(`Release tag must be v${manifest.version}.`)
}
if (process.platform !== 'win32' || process.arch !== 'x64') {
  throw new Error('This plugin release build supports Windows x86_64 MSVC only.')
}
if (manifest.platform?.os !== 'windows' || manifest.platform?.architecture !== 'x86_64' || manifest.platform?.abi !== 'msvc') {
  throw new Error('manifest.json does not target Windows x86_64 MSVC.')
}
if (
  manifest.capabilities.includes('network.public')
  && (!Array.isArray(manifest.networkPublicHosts) || manifest.networkPublicHosts.length === 0)
) {
  throw new Error('network.public requires explicit networkPublicHosts in manifest.json.')
}

function run(command, args, cwd = root) {
  const commandName = process.platform === 'win32' && command === 'pnpm'
    ? (process.env.ComSpec ?? 'cmd.exe')
    : command
  const commandArgs = commandName === command
    ? args
    : ['/d', '/s', '/c', ['pnpm.cmd', ...args].map((arg) => {
      if (!/^[\w./:-]+$/.test(arg)) throw new Error(`Unsupported Windows command argument: ${arg}`)
      return arg
    }).join(' ')]
  const result = spawnSync(commandName, commandArgs, { cwd, stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
}

run('pnpm', ['--dir', 'ui', 'run', 'typecheck'])
run('pnpm', ['--dir', 'ui', 'run', 'test'])
run('pnpm', ['--dir', 'ui', 'run', 'build'])
run('cargo', [
  'build', '--locked', '--release',
  '--package', cargoPackageName, '--bin', cargoPackageName,
  '--package', 'wonderland-mcp-knowledge', '--bin', 'mcp-knowledge',
])

const targetRoot = path.resolve(root, 'target')
const staging = path.resolve(targetRoot, 'knowledge_library-release-package')
const relativeStaging = path.relative(targetRoot, staging)
if (!relativeStaging || relativeStaging.startsWith('..') || path.isAbsolute(relativeStaging)) {
  throw new Error('Refusing to replace a release package outside this repository target directory.')
}
await rm(staging, { recursive: true, force: true })
await mkdir(path.join(staging, 'backend'), { recursive: true })
await cp(path.join(root, 'package/manifest.json'), path.join(staging, 'manifest.json'))
await cp(path.join(root, 'package/contract.json'), path.join(staging, 'contract.json'))
await cp(path.join(root, 'ui/dist'), path.join(staging, 'ui'), { recursive: true })

const backendEntry = manifest.backend?.entry
if (backendEntry !== 'backend/wonderland-knowledge-library.exe') {
  throw new Error('Unexpected plugin backend entry in manifest.json.')
}
await cp(path.join(targetRoot, 'release', `${cargoPackageName}.exe`), path.join(staging, ...backendEntry.split('/')))
await cp(path.join(targetRoot, 'release', 'mcp-knowledge.exe'), path.join(staging, 'backend', 'mcp-knowledge.exe'))

const files = await collectFiles(staging)
const checksums = {
  algorithm: 'sha256',
  files: await Promise.all(files.map(async (relativePath) => ({
    path: relativePath,
    sha256: createHash('sha256').update(await readFile(path.join(staging, ...relativePath.split('/')))).digest('hex'),
  }))),
}
await writeFile(path.join(staging, 'checksums.json'), `${JSON.stringify(checksums, null, 2)}\n`, 'utf8')

const archivePath = path.join(
  targetRoot,
  `${manifest.id}-${manifest.version}-windows-${manifest.platform.architecture}.wplug`,
)
await rm(archivePath, { force: true })
run('powershell.exe', [
  '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
  '-File', path.join(root, 'scripts/create-wplug.ps1'),
  '-SourceDirectory', staging,
  '-DestinationPath', archivePath,
])

const archiveBytes = await readFile(archivePath)
if (archiveBytes.length < 1 || archiveBytes.length > 100 * 1024 * 1024) {
  throw new Error('The .wplug archive must be between 1 byte and 100 MiB.')
}
console.log(`Release package created: ${archivePath}`)
console.log(`SHA-256: ${createHash('sha256').update(archiveBytes).digest('hex')}`)

async function collectFiles(directory, prefix = '') {
  const entries = await readdir(directory, { withFileTypes: true })
  const files = []
  for (const entry of entries) {
    const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name
    if (entry.isSymbolicLink()) throw new Error(`Release staging contains a symbolic link: ${relativePath}`)
    if (entry.isDirectory()) files.push(...await collectFiles(path.join(directory, entry.name), relativePath))
    else if (entry.isFile()) files.push(relativePath)
    else throw new Error(`Release staging contains an unsupported entry: ${relativePath}`)
  }
  return files.sort()
}
