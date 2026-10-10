import { useCallback, useEffect, useState } from 'react'

export const MIN_COLUMNS = 2
export const MAX_COLUMNS = 12
const DEFAULT_COLUMNS = 5
const STORAGE_KEY = 'gallery.columns'

type ColumnsUpdate = number | ((columns: number) => number)

function clampColumns(value: number): number {
  return Number.isInteger(value) ? Math.min(MAX_COLUMNS, Math.max(MIN_COLUMNS, value)) : DEFAULT_COLUMNS
}

/** Column count (zoom level) of the gallery, persisted across sessions. */
export function useColumns(): [number, (update: ColumnsUpdate) => void] {
  const [columns, setColumns] = useState(() => clampColumns(Number(localStorage.getItem(STORAGE_KEY) ?? NaN)))
  useEffect(() => localStorage.setItem(STORAGE_KEY, String(columns)), [columns])
  const update = useCallback(
    (value: ColumnsUpdate) =>
      setColumns((previous) => clampColumns(typeof value === 'function' ? value(previous) : value)),
    [],
  )
  return [columns, update]
}
