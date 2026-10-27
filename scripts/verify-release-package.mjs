#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { lstat, readFile, readdir } from 'node:fs/promises'
import path from 'node:path'

const packageRoot = process.argv[2] ? path.resolve(process.argv[2]) : ''
if (!packageRoot) throw new Error('Usage: node scripts/verify-release-package.mjs <extracted-package-directory>')

const manifest = JSON.parse(await readFile(path.join(packageRoot, 'manifest.json'), 'utf8'))
const checksums = JSON.parse(await readFile(path.join(packageRoot, 'checksums.json'), 'utf8'))
if (
  manifest.manifestVersion !== 3
  || manifest.id !== 'wla_knowledge_library'
  || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$/.test(manifest.version ?? '')
  || manifest.platform?.os !== 'windows'
  || manifest.platform?.architecture !== 'x86_64'
  || manifest.platform?.abi !== 'msvc'
  || manifest.backend?.entry !== 'backend/wla_knowledge_library.exe'
) {
  throw new Error('Release package contains invalid wla_knowledge_library manifest metadata.')
}
if (checksums.algorithm !== 'sha256' || !Array.isArray(checksums.files) || checksums.files.length === 0) {
  throw new Error('Release package contains an invalid checksums.json.')
}
const expectedEntries = [manifest.ui?.entry, manifest.backend?.entry, manifest.contract, 'backend/wla_knowledge_mcp.exe']
if (expectedEntries.some((entry) => typeof entry !== 'string')) {
  throw new Error('Release package manifest or MCP entry metadata is incomplete.')
}

async function collectFiles(directory, prefix = '') {
  const files = []
  const entries = await readdir(directory, { withFileTypes: true })
  entries.sort((left, right) => left.name.localeCompare(right.name))
  for (const entry of entries) {
    const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name
    const absolutePath = path.join(directory, entry.name)
    const info = await lstat(absolutePath)
    if (info.isSymbolicLink()) throw new Error(`Package contains a symbolic link: ${relativePath}`)
    if (entry.isDirectory()) files.push(...await collectFiles(absolutePath, relativePath))
    else if (entry.isFile() && relativePath !== 'checksums.json') files.push(relativePath)
    else if (!entry.isFile()) throw new Error(`Package contains an unsupported entry: ${relativePath}`)
  }
  return files.sort()
}

const actualFiles = await collectFiles(packageRoot)
for (const entry of expectedEntries) {
  if (!actualFiles.includes(entry)) throw new Error(`Manifest references a missing package file: ${entry}`)
}
const listedFiles = checksums.files.map((entry) => entry.path)
if (listedFiles.some((entry) => (
  typeof entry !== 'string'
  || !/^[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*$/.test(entry)
  || entry.split('/').some((part) => part === '.' || part === '..')
))) {
  throw new Error('checksums.json contains an invalid package path.')
}
if (new Set(listedFiles).size !== listedFiles.length) throw new Error('checksums.json contains duplicate paths.')
listedFiles.sort()
if (JSON.stringify(actualFiles) !== JSON.stringify(listedFiles)) {
  throw new Error('checksums.json does not cover exactly the package files.')
}
for (const entry of checksums.files) {
  if (!/^[a-f0-9]{64}$/.test(entry.sha256 ?? '')) throw new Error(`Invalid SHA-256 for ${entry.path}.`)
  const bytes = await readFile(path.join(packageRoot, ...entry.path.split('/')))
  const hash = createHash('sha256').update(bytes).digest('hex')
  if (hash !== entry.sha256) throw new Error(`Checksum mismatch for ${entry.path}.`)
}
console.log(`Verified wla_knowledge_library v${manifest.version} package checksums.`)
