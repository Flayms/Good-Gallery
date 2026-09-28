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

const MIME_TYPES: Record<string, string> = {
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.png': 'image/png',
  '.webp': 'image/webp',
  '.gif': 'image/gif',
  '.avif': 'image/avif',
  '.mp4': 'video/mp4',
  '.m4v': 'video/mp4',
  '.mov': 'video/quicktime',
  '.webm': 'video/webm',
  '.mkv': 'video/x-matroska',
}

// NAS / OS housekeeping folders (Synology, QNAP, Windows).
const IGNORED_DIRS = new Set(['@eadir', '#recycle', '#snapshot', '.@__thumb', 'system volume information'])

export function mediaKind(fileName: string): MediaKind | undefined {
  // Dotfiles include macOS AppleDouble files (`._IMG_1.jpg`), which aren't real media.
  if (fileName.startsWith('.')) return undefined
  return KINDS[extname(fileName).toLowerCase()]
}

export function mimeType(fileName: string): string {
  return MIME_TYPES[extname(fileName).toLowerCase()] ?? 'application/octet-stream'
}

export function isIgnoredDir(name: string): boolean {
  return name.startsWith('.') || name.startsWith('$') || IGNORED_DIRS.has(name.toLowerCase())
}

export function isSidecar(fileName: string): boolean {
  return extname(fileName).toLowerCase() === '.xmp'
}
