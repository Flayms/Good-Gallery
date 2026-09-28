import { thumbHashToDataURL } from 'thumbhash'

/** Decodes a base64 ThumbHash (as stored in the DB) into a tiny PNG data URL placeholder. */
export function thumbhashDataUrl(base64: string): string {
  return thumbHashToDataURL(Uint8Array.from(atob(base64), (char) => char.charCodeAt(0)))
}
