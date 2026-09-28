import { extname } from 'node:path'
import type { Media } from '../main/db/schema'

export type MediaKind = Media['kind']

const KINDS: Record<string, MediaKind> = {
  '.jpg': 'image',
  '.jpeg': 'image',
  '.png': 'image',
  '.webp': 'image',
  '.gif': 'image',
  '.avif': 'image',
  '.mp4': 'video',
  '.m4v': 'video',
  '.mov': 'video',
  '.webm': 'video',
  '.mkv': 'video',
}

// NAS / OS housekeeping folders (Synology, QNAP, Windows).
const IGNORED_DIRS = new Set(['@eadir', '#recycle', '#snapshot', '.@__thumb', 'system volume information'])

export function mediaKind(fileName: string): MediaKind | undefined {
  // Dotfiles include macOS AppleDouble files (`._IMG_1.jpg`), which aren't real media.
  if (fileName.startsWith('.')) return undefined
  return KINDS[extname(fileName).toLowerCase()]
}

export function isIgnoredDir(name: string): boolean {
  return name.startsWith('.') || name.startsWith('$') || IGNORED_DIRS.has(name.toLowerCase())
}

export function isSidecar(fileName: string): boolean {
  return extname(fileName).toLowerCase() === '.xmp'
}
