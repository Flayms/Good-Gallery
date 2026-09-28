import { useEffect, useState } from 'react'

export const MIN_COLUMNS = 2
export const MAX_COLUMNS = 12
const DEFAULT_COLUMNS = 5
const STORAGE_KEY = 'gallery.columns'

function clampColumns(value: number): number {
  return Number.isInteger(value) ? Math.min(MAX_COLUMNS, Math.max(MIN_COLUMNS, value)) : DEFAULT_COLUMNS
}

/** Column count (zoom level) of the gallery, persisted across sessions. */
export function useColumns(): [number, (columns: number) => void] {
  const [columns, setColumns] = useState(() => clampColumns(Number(localStorage.getItem(STORAGE_KEY) ?? NaN)))
  useEffect(() => localStorage.setItem(STORAGE_KEY, String(columns)), [columns])
  return [columns, (value) => setColumns(clampColumns(value))]
}
